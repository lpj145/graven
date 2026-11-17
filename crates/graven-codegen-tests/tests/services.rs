use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use graven::rpc::{
    decode_fault, decode_frame, encode_end, encode_frame, fault_frame_bytes, BytesMut, CallError, Fault,
    FaultCode, FrameHeader, FrameKind, OwnedBrokenRule, OwnedFault, ProtocolError, ReplyStream, Response,
    Sender, StreamError, StreamResponse,
};
use graven_codegen_tests::inventory::{
    inventory_timeouts, metrics_timeouts, ActionOutVerified, AddCommandW, Checks,
    CommandVerified, EventW, GetReqW, InventoryCall, InventoryClient, ItemW, MetricsCall,
    MetricsClient, ReserveErrVerified, INVENTORY_SERVICE_ID, METRICS_SERVICE_ID,
};

struct NoChecks;
impl Checks for NoChecks {}

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = Box::pin(fut);
    let mut cx = Context::from_waker(Waker::noop());
    match Pin::as_mut(&mut fut).poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("in-memory test future unexpectedly yielded Pending"),
    }
}

/// Reads a stream to its end.
async fn collect<T: graven::rpc::RpcView, R: ReplyStream>(
    s: &mut StreamResponse<T, R>,
) -> Vec<Result<Response<T>, StreamError<R::Error>>> {
    let mut out = Vec::new();
    while let Some(r) = s.next().await {
        out.push(r);
    }
    out
}

struct Loopback<F> {
    handler: F,
    timeouts: RefCell<Vec<Option<u32>>>,
}

impl<F> Loopback<F> {
    fn new(handler: F) -> Self {
        Self { handler, timeouts: RefCell::new(Vec::new()) }
    }

    fn take_timeouts(&self) -> Vec<Option<u32>> {
        std::mem::take(&mut *self.timeouts.borrow_mut())
    }
}

impl<F> Sender for Loopback<F>
where
    F: Fn(&[u8]) -> Result<Vec<u8>, &'static str>,
{
    type Error = &'static str;

    fn send(
        &self,
        call: Vec<u8>,
        timeout_ms: Option<u32>,
    ) -> impl Future<Output = Result<Vec<u8>, Self::Error>> {
        self.timeouts.borrow_mut().push(timeout_ms);
        let res = (self.handler)(&call);
        async move { res }
    }
}

fn dispatch_inventory_validated(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    match InventoryCall::decode_validated(bytes, &NoChecks) {
        Ok(call) => match call {
            InventoryCall::GetItem(input, reply) => {
                let sku = input.sku().to_owned();
                Ok(reply.ok(|w| {
                    w.sku(&sku).stock(42);
                }))
            }
            InventoryCall::Reserve(input, reply) => {
                let sku = input.sku();
                let qty = input.qty();
                if sku == "unknown" {
                    Ok(reply.err(|e| {
                        e.invalid_sku().reason("unknown sku");
                    }))
                } else if qty > 10 {
                    Ok(reply.err(|e| {
                        e.out_of_stock().available(10);
                    }))
                } else {
                    Ok(reply.ok(|w| {
                        w.id(9001);
                    }))
                }
            }
            InventoryCall::Watch(input, mut reply) => {
                let sku = input.sku();
                if sku == "fault-mid-stream" {
                    let mut buf = reply.item(|w| {
                        w.seq(1).stock(50);
                    });
                    buf.extend_from_slice(&fault_frame_bytes(&Fault {
                            code: FaultCode::Unavailable,
                            message: Some("replica lost"),
                            broken: None,
                        }));
                    Ok(buf)
                } else {
                    let mut buf = reply.item(|w| {
                        w.seq(1).stock(10);
                    });
                    buf.extend_from_slice(&reply.item(|w| {
                        w.seq(2).stock(8);
                    }));
                    buf.extend_from_slice(&reply.end());
                    Ok(buf)
                }
            }
            InventoryCall::Batch(input, mut reply) => {
                let count = input.count();
                if count == 99 {
                    Ok(reply.err(|e| {
                        e.code(503);
                    }))
                } else {
                    let mut buf = Vec::new();
                    for i in 0..count {
                        buf.extend_from_slice(&reply.item(|w| {
                            w.index(i);
                        }));
                    }
                    buf.extend_from_slice(&reply.end());
                    Ok(buf)
                }
            }
            InventoryCall::Audit(_) => Ok(Vec::new()),
            InventoryCall::Dispatch(input, reply) => match input {
                CommandVerified::Add(cmd) => {
                    let delta = cmd.delta();
                    Ok(reply.ok(|o| {
                        o.added().new_stock((100 + delta) as u32);
                    }))
                }
                CommandVerified::Clear(_) => Ok(reply.ok(|o| {
                    o.cleared().removed(100);
                })),
                CommandVerified::Unknown(_) => Ok(reply.err(|e| {
                    e.available(0);
                })),
            },
        },
        Err(rej) => Ok(rej.to_frame().unwrap_or_default()),
    }
}

#[test]
fn constants_and_timeouts_match_schema() {
    assert_eq!(inventory_timeouts::GET_ITEM_MS, 2500);
    assert_eq!(inventory_timeouts::WATCH_MS, 10000);
    assert_eq!(metrics_timeouts::PING_MS, 5000);
    assert_ne!(INVENTORY_SERVICE_ID, METRICS_SERVICE_ID);
}

#[test]
fn roundtrips_all_five_shapes_and_unions() {
    let t = Loopback::new(dispatch_inventory_validated);
    let client = InventoryClient::new(&t);

    // 1. Unary infallible + default timeout + override
    let resp = block_on(client.get_item(|w| {
        w.sku("widget-1");
    }))
    .unwrap();
    assert_eq!(resp.view().sku(), "widget-1");
    assert_eq!(resp.view().stock(), 42);
    assert_eq!(t.take_timeouts(), vec![Some(2500)]);

    let resp = block_on(client.get_item_with_timeout(
        |w| {
            w.sku("widget-2");
        },
        Some(999),
    ))
    .unwrap();
    assert_eq!(resp.view().sku(), "widget-2");
    assert_eq!(t.take_timeouts(), vec![Some(999)]);

    // 2. Unary Result: Ok & Err union variants
    let ok = block_on(client.reserve(|w| {
        w.sku("widget-1").qty(5);
    }))
    .unwrap();
    assert_eq!(ok.view().id(), 9001);
    assert_eq!(t.take_timeouts(), vec![None]);

    let err = block_on(client.reserve(|w| {
        w.sku("widget-1").qty(20);
    }))
    .unwrap_err();
    assert_eq!(t.take_timeouts(), vec![None]);
    match err {
        CallError::Err(e) => match e.view() {
            ReserveErrVerified::OutOfStock(oos) => assert_eq!(oos.available(), 10),
            ReserveErrVerified::InvalidSku(_) => panic!("unexpected InvalidSku"),
            ReserveErrVerified::Unknown(tag) => panic!("unexpected Unknown({tag})"),
        },
        other => panic!("expected CallError::Err, got {other:?}"),
    }

    // 3. Stream infallible
    let mut stream = block_on(client.watch(|w| {
        w.sku("widget-1");
    }))
    .unwrap();
    assert_eq!(t.take_timeouts(), vec![Some(10000)]);
    let events: Vec<(u64, u32)> = block_on(collect(&mut stream))
        .into_iter()
        .map(|r| {
            let v = r.unwrap();
            (v.view().seq(), v.view().stock())
        })
        .collect();
    assert_eq!(events, vec![(1, 10), (2, 8)]);

    // 4. Stream Result: Ok items + End
    let mut stream = block_on(client.batch(|w| {
        w.count(3);
    }))
    .unwrap();
    let indices: Vec<u32> = block_on(collect(&mut stream)).into_iter().map(|r| r.unwrap().view().index()).collect();
    assert_eq!(indices, vec![0, 1, 2]);

    // 4b. Stream Result: immediate Err before items
    let err = block_on(client.batch(|w| {
        w.count(99);
    }));
    match err {
        Err(CallError::Err(e)) => assert_eq!(e.view().code(), 503),
        Ok(_) => panic!("expected CallError::Err, got Ok stream"),
        Err(other) => panic!("expected CallError::Err, got {other:?}"),
    }

    // 5. One-way call
    block_on(client.audit(|w| {
        w.actor("system").note("restocked");
    }))
    .unwrap();

    // 6. Union input & union output
    let resp = block_on(client.dispatch(|cmd| {
        cmd.add().sku("widget-1").delta(15);
    }))
    .unwrap();
    match resp.view() {
        ActionOutVerified::Added(a) => assert_eq!(a.new_stock(), 115),
        ActionOutVerified::Cleared(_) => panic!("unexpected Cleared"),
        ActionOutVerified::Unknown(tag) => panic!("unexpected Unknown({tag})"),
    }
    let resp = block_on(client.dispatch(|cmd| {
        cmd.clear().sku("widget-1");
    }))
    .unwrap();
    match resp.view() {
        ActionOutVerified::Cleared(c) => assert_eq!(c.removed(), 100),
        ActionOutVerified::Added(_) => panic!("unexpected Added"),
        ActionOutVerified::Unknown(tag) => panic!("unexpected Unknown({tag})"),
    }
}

#[test]
fn service_level_timeout_applies_to_methods() {
    let t = Loopback::new(|bytes: &[u8]| match MetricsCall::decode(bytes) {
        Ok(MetricsCall::Record(_)) => Ok(Vec::new()),
        Ok(MetricsCall::Ping(input, reply)) => {
            let count = input.count();
            Ok(reply.ok(|w| {
                w.index(count);
            }))
        }
        Err(rej) => Ok(rej.to_frame().unwrap_or_default()),
    });
    let client = MetricsClient::new(&t);

    block_on(client.record(|w| {
        w.actor("bot").note("ok");
    }))
    .unwrap();
    let ping = block_on(client.ping(|w| {
        w.count(7);
    }))
    .unwrap();
    assert_eq!(ping.view().index(), 7);
    // record is one-way (no timeout passed), ping inherits service-level 5000ms.
    assert_eq!(t.take_timeouts(), vec![None, Some(5000)]);
}

#[test]
fn server_edge_rejections_and_validation_faults() {
    // 1. Client receives FaultCode::Invalid with BrokenRule from decode_validated
    let t = Loopback::new(dispatch_inventory_validated);
    let client = InventoryClient::new(&t);

    let err = block_on(client.get_item(|w| {
        w.sku(""); // violates #[len(min = 1, max = 32)]
    }))
    .unwrap_err();
    assert_eq!(
        err,
        CallError::Fault(OwnedFault {
            code: FaultCode::Invalid,
            message: None,
            broken: Some(Box::new(OwnedBrokenRule {
                table_name: "GetReq".to_owned(),
                field: Some("sku".to_owned()),
                index: None,
                rule: "len".to_owned(),
                min: Some("1".to_owned()),
                max: Some("32".to_owned()),
                reason: None,
            })),
        })
    );

    let err = block_on(client.batch(|w| {
        w.count(500); // violates #[range(min = 1, max = 100)]
    }));
    match err {
        Err(CallError::Fault(f)) => {
            assert_eq!(f.code, FaultCode::Invalid);
            assert_eq!(f.broken.as_ref().unwrap().rule, "range");
        }
        Ok(_) => panic!("expected Fault"),
        Err(other) => panic!("expected Fault, got {other:?}"),
    }

    // 2. Wrong service_id -> Unsupported
    let bad_svc = graven::rpc::encode_call_frame::<GetReqW<'static>>(0xDEAD_BEEF, 1, |w| {
        w.sku("a");
    });
    let rej = match InventoryCall::decode(&bad_svc) {
        Err(r) => r,
        Ok(_) => panic!("expected rejection"),
    };
    assert_eq!(rej.code(), FaultCode::Unsupported);
    let fbytes = rej.to_frame().expect("fault frame for wrong service_id");
    let frame = decode_frame(&fbytes).unwrap();
    assert_eq!(decode_fault(frame.payload).unwrap().code, FaultCode::Unsupported);

    // 3. Unknown and @reserved method ids -> Unsupported
    for mid in [6u16, 99u16] {
        let call = graven::rpc::encode_call_frame::<GetReqW<'static>>(INVENTORY_SERVICE_ID, mid, |w| {
            w.sku("a");
        });
        let rej = match InventoryCall::decode(&call) {
            Err(r) => r,
            Ok(_) => panic!("expected rejection for method {mid}"),
        };
        assert_eq!(rej.code(), FaultCode::Unsupported);
        assert!(rej.to_frame().is_some());
    }

    // 4. Malformed frame (wrong kind) -> Malformed
    let mut bad_kind = BytesMut::new();
    encode_end(&mut bad_kind);
    let rej = match InventoryCall::decode(&bad_kind) {
        Err(r) => r,
        Ok(_) => panic!("expected rejection for wrong kind"),
    };
    assert_eq!(rej.code(), FaultCode::Malformed);

    // 5. One-way call failure produces no reply frame (`to_frame() == None`)
    let bad_oneway = graven::rpc::encode_call_frame::<graven_codegen_tests::inventory::AuditLogW<'static>>(
        INVENTORY_SERVICE_ID,
        5,
        |w| {
            w.actor("").note("bad"); // empty actor violates len(min = 1)
        },
    );
    let rej = match InventoryCall::decode_validated(&bad_oneway, &NoChecks) {
        Err(r) => r,
        Ok(_) => panic!("expected validation failure on one-way"),
    };
    assert_eq!(rej.code(), FaultCode::Invalid);
    assert_eq!(rej.to_frame(), None);

    // 6. Unknown union variant in call payload decodes cleanly as Unknown(tag)
    let mut raw_env = BytesMut::new();
    let sp = graven::SimplePool::new(512);
    let mut sb = graven::Builder::new_enveloped(&sp);
    let env = AddCommandW::to_bytes(&mut sb, |w| {
        w.sku("k").delta(1);
    })
    .unwrap();
    encode_frame(FrameHeader::call(INVENTORY_SERVICE_ID, 7, 77), &env[20..env.len() - 4], &mut raw_env).unwrap();
    match InventoryCall::decode_validated(&raw_env, &NoChecks).unwrap() {
        InventoryCall::Dispatch(input, _) => match input {
            CommandVerified::Unknown(tag) => assert_eq!(tag, 77),
            CommandVerified::Add(_) | CommandVerified::Clear(_) => panic!("expected Unknown(77)"),
        },
        _ => panic!("expected Dispatch"),
    }
}

#[test]
fn client_reports_protocol_and_stream_errors() {
    // 1. Mid-stream Fault after one valid Item
    let t = Loopback::new(dispatch_inventory_validated);
    let client = InventoryClient::new(&t);
    let mut stream = block_on(client.watch(|w| {
        w.sku("fault-mid-stream");
    }))
    .unwrap();
    assert_eq!(block_on(stream.next()).unwrap().unwrap().view().seq(), 1);
    match block_on(stream.next()).unwrap() {
        Err(StreamError::Fault(f)) => assert_eq!(f.code, FaultCode::Unavailable),
        other => panic!("expected StreamError::Fault, got {other:?}"),
    }
    assert!(block_on(stream.next()).is_none());

    // 2. A reply message that is not a valid Item -> Payload
    let bad_message = Loopback::new(|_: &[u8]| {
        let mut out = BytesMut::new();
        encode_frame(FrameHeader::reply(FrameKind::Ok, 0), &[0xff, 0xff], &mut out).unwrap();
        Ok(out.to_vec())
    });
    let err = block_on(InventoryClient::new(&bad_message).get_item(|w| {
        w.sku("x");
    }))
    .unwrap_err();
    assert!(matches!(err, CallError::Protocol(ProtocolError::Payload(_))), "{err:?}");

    // 3. A union variant on a table reply -> UnexpectedVariant
    let bad_variant = Loopback::new(|_: &[u8]| {
        let sp = graven::SimplePool::new(512);
        let mut b = graven::Builder::new_enveloped(&sp);
        let env = ItemW::to_bytes(&mut b, |w| {
            w.sku("x").stock(1);
        })
        .unwrap();
        let mut out = BytesMut::new();
        encode_frame(FrameHeader::reply(FrameKind::Ok, 3), &env[20..env.len() - 4], &mut out).unwrap();
        Ok(out.to_vec())
    });
    let err = block_on(InventoryClient::new(&bad_variant).get_item(|w| {
        w.sku("x");
    }))
    .unwrap_err();
    assert_eq!(err, CallError::Protocol(ProtocolError::UnexpectedVariant(3)));

    // 4. Trailing bytes after unary Ok -> TrailingBytes
    let trailing = Loopback::new(|call: &[u8]| {
        let mut rep = dispatch_inventory_validated(call)?;
        rep.extend_from_slice(&[0u8; 4]);
        Ok(rep)
    });
    let err = block_on(InventoryClient::new(&trailing).get_item(|w| {
        w.sku("x");
    }))
    .unwrap_err();
    assert_eq!(err, CallError::Protocol(ProtocolError::TrailingBytes(4)));

    // 5. Wrong kind on streaming call (Ok instead of Item/End) -> UnexpectedKind
    let wrong_kind = Loopback::new(|_: &[u8]| {
        let sp = graven::SimplePool::new(512);
        let mut b = graven::Builder::new_enveloped(&sp);
        let env = EventW::to_bytes(&mut b, |w| {
            w.seq(1).stock(1);
        })
        .unwrap();
        let mut out = BytesMut::new();
        encode_frame(FrameHeader::reply(FrameKind::Ok, 0), &env[20..env.len() - 4], &mut out).unwrap();
        Ok(out.to_vec())
    });
    match block_on(InventoryClient::new(&wrong_kind).watch(|w| {
        w.sku("x");
    })) {
        Err(CallError::Protocol(ProtocolError::UnexpectedKind(FrameKind::Ok))) => {}
        Ok(_) => panic!("expected UnexpectedKind"),
        Err(other) => panic!("expected UnexpectedKind, got {other:?}"),
    }

    // 6. Transport failure surfaces as CallError::Send
    let broken = Loopback::new(|_: &[u8]| Err("connection reset"));
    let err = block_on(InventoryClient::new(&broken).get_item(|w| {
        w.sku("x");
    }))
    .unwrap_err();
    assert_eq!(err, CallError::Send("connection reset"));
}

/// A reply delivered a few bytes at a time, recording how far the server has sent.
struct Trickle {
    bytes: Vec<u8>,
    pos: std::rc::Rc<std::cell::Cell<usize>>,
    chunk: usize,
    fail_at: Option<usize>,
}

impl ReplyStream for Trickle {
    type Error = &'static str;

    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        let pos = self.pos.get();
        if self.fail_at.is_some_and(|at| pos >= at) {
            return Err("connection reset");
        }
        if pos >= self.bytes.len() {
            return Ok(None);
        }
        let end = (pos + self.chunk).min(self.bytes.len());
        self.pos.set(end);
        Ok(Some(self.bytes[pos..end].to_vec()))
    }
}

/// A sender whose streaming replies arrive in small chunks.
struct Live {
    reply: fn(&[u8]) -> Vec<u8>,
    pos: std::rc::Rc<std::cell::Cell<usize>>,
    fail_at: Option<usize>,
}

impl Sender for Live {
    type Error = &'static str;

    async fn send(&self, call: Vec<u8>, _timeout_ms: Option<u32>) -> Result<Vec<u8>, Self::Error> {
        Ok((self.reply)(&call))
    }

    async fn send_stream(&self, call: Vec<u8>, _timeout_ms: Option<u32>) -> Result<impl ReplyStream<Error = Self::Error>, Self::Error> {
        self.pos.set(0);
        Ok(Trickle { bytes: (self.reply)(&call), pos: self.pos.clone(), chunk: 7, fail_at: self.fail_at })
    }
}

fn watch_reply(call: &[u8]) -> Vec<u8> {
    dispatch_inventory_validated(call).unwrap()
}

#[test]
fn stream_items_arrive_before_the_reply_ends() {
    let live = Live { reply: watch_reply, pos: Default::default(), fail_at: None };
    let client = InventoryClient::new(&live);
    let mut stream = block_on(client.watch(|w| {
        w.sku("widget-1");
    }))
    .unwrap();
    let first = block_on(stream.next()).unwrap().unwrap();
    assert_eq!(first.view().seq(), 1);
    let after_first = live.pos.get();
    assert_eq!(block_on(stream.next()).unwrap().unwrap().view().seq(), 2);
    assert!(block_on(stream.next()).is_none());
    assert!(after_first < live.pos.get(), "the first item was read before the whole reply arrived");
}

#[test]
fn stream_reports_a_cut_reply_and_a_failing_source() {
    // The reply stops after the first item, with no End frame.
    fn cut(call: &[u8]) -> Vec<u8> {
        let full = dispatch_inventory_validated(call).unwrap();
        let first = decode_frame(&full).unwrap().len();
        full[..first + 5].to_vec()
    }
    let live = Live { reply: cut, pos: Default::default(), fail_at: None };
    let client = InventoryClient::new(&live);
    let mut stream = block_on(client.watch(|w| {
        w.sku("widget-1");
    }))
    .unwrap();
    assert!(block_on(stream.next()).unwrap().is_ok());
    assert!(matches!(block_on(stream.next()), Some(Err(StreamError::Protocol(ProtocolError::Truncated)))));
    assert!(block_on(stream.next()).is_none());

    // The source fails after delivering the first frame, in the middle of the second.
    let call = graven::rpc::encode_call_frame::<graven_codegen_tests::inventory::WatchReqW<'static>>(
        graven_codegen_tests::inventory::INVENTORY_SERVICE_ID,
        3,
        |w| {
            w.sku("widget-1");
        },
    );
    let first_len = decode_frame(&watch_reply(&call)).unwrap().len();
    let live = Live { reply: watch_reply, pos: Default::default(), fail_at: Some(first_len + 3) };
    let client = InventoryClient::new(&live);
    let mut stream = block_on(client.watch(|w| {
        w.sku("widget-1");
    }))
    .unwrap();
    let mut results = block_on(collect(&mut stream));
    assert!(matches!(results.pop(), Some(Err(StreamError::Send("connection reset")))));
    assert!(results.iter().all(Result::is_ok));
}

#[test]
fn broken_frame_for_a_one_way_method_gets_no_reply() {
    let mut call = graven::rpc::encode_call_frame::<graven_codegen_tests::inventory::AuditLogW<'static>>(
        INVENTORY_SERVICE_ID,
        5,
        |w| {
            w.actor("a").note("b");
        },
    );
    let last = call.len() - 1;
    call[last] ^= 0xff; // CRC no longer matches
    let rej = match InventoryCall::decode(&call) {
        Err(r) => r,
        Ok(_) => panic!("expected rejection"),
    };
    assert_eq!(rej.code(), FaultCode::Malformed);
    assert_eq!(rej.to_frame(), None);

    // The same broken frame for a unary method is answered with a fault.
    let mut call = graven::rpc::encode_call_frame::<GetReqW<'static>>(INVENTORY_SERVICE_ID, 1, |w| {
        w.sku("a");
    });
    let last = call.len() - 1;
    call[last] ^= 0xff;
    match InventoryCall::decode(&call) {
        Err(r) => assert!(r.to_frame().is_some()),
        Ok(_) => panic!("expected rejection"),
    }
}

#[test]
fn stream_reply_frames_encodes_items_then_end_lazily() {
    use graven::rpc::{FrameKind, StreamReply, frames};
    use graven_codegen_tests::inventory::Event;
    let mut made = 0;
    let it = StreamReply::<EventW<'static>, ()>::new().frames(1..=3u64, |w, seq| {
        made += 1;
        w.seq(seq).stock(1);
    });
    let all: Vec<u8> = it.flatten().collect();
    assert_eq!(made, 3);
    let decoded: Vec<_> = frames(&all).collect::<Result<_, _>>().unwrap();
    let kinds: Vec<FrameKind> = decoded.iter().map(|f| f.header.kind).collect();
    assert_eq!(kinds, [FrameKind::Item, FrameKind::Item, FrameKind::Item, FrameKind::End]);
    let seqs: Vec<u64> = decoded[..3].iter().map(|f| Event::verified(f.payload).unwrap().seq()).collect();
    assert_eq!(seqs, [1, 2, 3]);

    // Lazy: nothing is encoded before the iterator is pulled.
    let mut pulled = 0;
    let mut lazy = StreamReply::<EventW<'static>, ()>::new().frames(0..2u64, |w, seq| {
        pulled += 1;
        w.seq(seq).stock(0);
    });
    let first = lazy.next().unwrap();
    assert!(!first.is_empty());
    drop(lazy);
    assert_eq!(pulled, 1);
}
