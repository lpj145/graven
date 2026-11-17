//! A Graven service served by Axum: one endpoint, bytes in, bytes out.
//!
//! `cargo run` listens on 127.0.0.1:3001; `node web/client.mjs` calls it with the generated
//! JavaScript client (see README.md).

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Arc, RwLock};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;

mod shop {
    include!(concat!(env!("OUT_DIR"), "/shop.rs"));
}
use shop::ShopCall;

/// What the service stores. In a real product: a database.
#[derive(Clone)]
struct Product {
    name: String,
    stock: u32,
}

type Db = Arc<RwLock<BTreeMap<String, Product>>>;

/// The largest call the server reads. A call is one frame: 17 bytes plus the message.
const MAX_CALL_BYTES: usize = 64 * 1024;

fn app(db: Db) -> Router {
    Router::new()
        .route("/shop", post(rpc))
        .layer(DefaultBodyLimit::max(MAX_CALL_BYTES))
        .with_state(db)
}

/// Graven frames always travel as `200 OK` with an opaque body: a declared error or a fault
/// is in the bytes, not in the HTTP status (HTTP statuses stay for what happens before the
/// call is read: authentication, limits, routing).
fn frames(body: impl Into<Body>) -> Response {
    ([(header::CONTENT_TYPE, "application/octet-stream")], body.into()).into_response()
}

async fn rpc(State(db): State<Db>, body: Bytes) -> Response {
    // Decodes the frame, checks it is a call to this service, verifies the input once and
    // runs the schema's validation rules (`&()`: this schema has no `check(name)`).
    let call = match ShopCall::decode_validated(&body, &()) {
        Ok(call) => call,
        // Malformed, unknown method or invalid input: a `Fault` frame, or nothing at all
        // for a one-way method.
        Err(rejected) => return frames(rejected.to_frame().unwrap_or_default()),
    };
    match call {
        ShopCall::Get(req, reply) => {
            let sku = req.sku(); // a verified view: no `?`, no error to handle
            let db = db.read().unwrap();
            frames(match db.get(sku) {
                Some(p) => reply.ok(|w| {
                    w.sku(sku).name(&p.name).stock(p.stock);
                }),
                None => reply.err(|e| {
                    e.not_found().sku(sku);
                }),
            })
        }
        ShopCall::List(req, reply) => {
            // Read what the stream needs now; `req` borrows the request body.
            let prefix = req.prefix().unwrap_or("").to_string();
            let rows: Vec<(String, Product)> = db
                .read()
                .unwrap()
                .range(prefix.clone()..)
                .take_while(|(sku, _)| sku.starts_with(&prefix))
                .take(req.limit() as usize)
                .map(|(sku, p)| (sku.clone(), p.clone()))
                .collect();
            // One `Item` frame per product, then `End`, encoded as the body is sent: the
            // client reads each item as soon as its bytes arrive.
            let items = reply.frames(rows, |w, (sku, p)| {
                w.sku(&sku).name(&p.name).stock(p.stock);
            });
            frames(Body::from_stream(futures_util::stream::iter(items.map(|f| Ok::<_, Infallible>(Bytes::from(f))))))
        }
        ShopCall::Adjust(req, reply) => {
            let sku = req.sku();
            let mut db = db.write().unwrap();
            let Some(p) = db.get_mut(sku) else {
                return frames(reply.err(|e| {
                    e.not_found().sku(sku);
                }));
            };
            let stock = i64::from(p.stock) + i64::from(req.delta());
            if stock < 0 {
                return frames(reply.err(|e| {
                    e.out_of_stock().sku(sku).available(p.stock);
                }));
            }
            p.stock = stock as u32;
            frames(reply.ok(|w| {
                w.sku(sku).name(&p.name).stock(p.stock);
            }))
        }
    }
}

fn sample_db() -> Db {
    Arc::new(RwLock::new(BTreeMap::from([
        ("cafe-1kg".to_string(), Product { name: "Café 1 kg".into(), stock: 3 }),
        ("cafe-500g".to_string(), Product { name: "Café 500 g".into(), stock: 12 }),
        ("cha-mate".to_string(), Product { name: "Chá mate".into(), stock: 40 }),
    ])))
}

#[tokio::main]
async fn main() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await.unwrap();
    println!("listening on http://127.0.0.1:3001/shop");
    axum::serve(listener, app(sample_db())).await.unwrap();
}

#[cfg(test)]
mod tests {
    //! The generated Rust client calls the Axum router in-process (no network): the same
    //! bytes a browser or another service would send.
    use super::*;
    use axum::http::Request;
    use graven::rpc::{CallError, FaultCode, Sender};
    use http_body_util::BodyExt;
    use shop::{LookupErrorVerified, ShopClient, StockErrorVerified};
    use tower::ServiceExt;

    struct InProcess(Router);

    impl Sender for InProcess {
        type Error = Infallible;
        async fn send(&self, call: Vec<u8>, _timeout_ms: Option<u32>) -> Result<Vec<u8>, Infallible> {
            Ok(post_raw(&self.0, call).await)
        }
    }

    async fn post_raw(app: &Router, body: Vec<u8>) -> Vec<u8> {
        let req = Request::post("/shop").body(Body::from(body)).unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()[header::CONTENT_TYPE], "application/octet-stream");
        res.into_body().collect().await.unwrap().to_bytes().to_vec()
    }

    fn client() -> ShopClient<InProcess> {
        ShopClient::new(InProcess(app(sample_db())))
    }

    #[tokio::test]
    async fn get_returns_the_product_or_a_typed_error() {
        let shop = client();
        let p = shop.get(|w| { w.sku("cafe-500g"); }).await.unwrap();
        assert_eq!((p.view().name(), p.view().stock()), ("Café 500 g", 12));

        match shop.get(|w| { w.sku("chá"); }).await {
            Err(CallError::Err(e)) => match e.view() {
                LookupErrorVerified::NotFound(nf) => assert_eq!(nf.sku(), "chá"),
                LookupErrorVerified::Unknown(tag) => panic!("unknown variant {tag}"),
            },
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_streams_matching_products() {
        let shop = client();
        let mut items = shop.list(|w| { w.prefix("cafe"); }).await.unwrap();
        let mut skus = Vec::new();
        while let Some(p) = items.next().await {
            skus.push(p.unwrap().view().sku().to_string());
        }
        assert_eq!(skus, ["cafe-1kg", "cafe-500g"]);
    }

    #[tokio::test]
    async fn adjust_updates_stock_or_refuses() {
        let shop = client();
        let p = shop.adjust(|w| { w.sku("cafe-1kg").delta(2); }).await.unwrap();
        assert_eq!(p.view().stock(), 5);

        match shop.adjust(|w| { w.sku("cafe-1kg").delta(-9); }).await {
            Err(CallError::Err(e)) => match e.view() {
                StockErrorVerified::OutOfStock(o) => assert_eq!(o.available(), 5),
                _ => panic!("expected OutOfStock"),
            },
            other => panic!("expected OutOfStock, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_input_is_a_fault_with_codes() {
        // `delta` is declared `#[range(min = -1000, max = 1000)]`.
        match client().adjust(|w| { w.sku("cafe-1kg").delta(5000); }).await {
            Err(CallError::Fault(f)) => {
                assert_eq!(f.code, FaultCode::Invalid);
                assert_eq!(f.message, None);
                let b = f.broken.unwrap();
                assert_eq!((b.field.as_deref(), b.rule.as_str()), (Some("delta"), "range"));
                assert_eq!((b.min.as_deref(), b.max.as_deref()), (Some("-1000"), Some("1000")));
            }
            other => panic!("expected an Invalid fault, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn garbage_is_a_malformed_fault() {
        let reply = post_raw(&app(sample_db()), b"not a frame".to_vec()).await;
        let frame = graven::rpc::decode_frame(&reply).unwrap();
        let fault = graven::rpc::decode_fault(frame.payload).unwrap();
        assert_eq!(fault.code, FaultCode::Malformed);
    }
}
