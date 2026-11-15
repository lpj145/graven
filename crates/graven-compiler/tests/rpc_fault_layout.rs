//! `graven::rpc` encodes and decodes `graven.rpc.Fault` by hand, with fixed offsets. This test
//! checks those bytes against what the compiler lays out for the published schema
//! (`crates/graven/schemas/rpc.graven`), in both directions.

use std::collections::BTreeMap;
use std::path::PathBuf;

use graven::rpc::{BrokenRule, FAULT_SCHEMA_ID, BROKEN_RULE_SCHEMA_ID, Fault, FaultCode, decode_fault, encode_fault_payload};
use graven_compiler::dynamic::Schema;
use graven_dynamic::Value;

fn schema() -> Schema {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../graven/schemas/rpc.graven");
    Schema::load(&path).expect("graven.rpc schema and lock")
}

fn table(fields: &[(&str, Value)]) -> Value {
    Value::Table(fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

fn cases() -> Vec<(Fault<'static>, Value)> {
    vec![
        (
            Fault { code: FaultCode::Unsupported, message: None, broken: None },
            table(&[("code", Value::UInt(1))]),
        ),
        (
            Fault { code: FaultCode::Unavailable, message: Some("replica lost"), broken: None },
            table(&[("code", Value::UInt(6)), ("message", Value::Str("replica lost".into()))]),
        ),
        (
            Fault {
                code: FaultCode::Invalid,
                message: Some("Item.qty: breaks rule `range`"),
                broken: Some(BrokenRule { table_name: "Item", field: Some("qty"), index: Some(3), rule: "range" }),
            },
            table(&[
                ("code", Value::UInt(3)),
                ("message", Value::Str("Item.qty: breaks rule `range`".into())),
                (
                    "broken",
                    table(&[
                        ("table_name", Value::Str("Item".into())),
                        ("field", Value::Str("qty".into())),
                        ("index", Value::UInt(3)),
                        ("rule", Value::Str("range".into())),
                    ]),
                ),
            ]),
        ),
        (
            Fault {
                code: FaultCode::Unknown(99),
                message: None,
                broken: Some(BrokenRule { table_name: "Promo", field: None, index: None, rule: "check(valid_period)" }),
            },
            table(&[
                ("code", Value::UInt(99)),
                (
                    "broken",
                    table(&[("table_name", Value::Str("Promo".into())), ("rule", Value::Str("check(valid_period)".into()))]),
                ),
            ]),
        ),
    ]
}

#[test]
fn schema_ids_match_the_published_schema() {
    let s = schema();
    assert_eq!(s.schema_id(s.table("Fault").unwrap()), FAULT_SCHEMA_ID);
    assert_eq!(s.schema_id(s.table("BrokenRule").unwrap()), BROKEN_RULE_SCHEMA_ID);
}

#[test]
fn runtime_fault_bytes_read_as_the_schema_says() {
    let s = schema();
    let model = s.model(s.table("Fault").unwrap());
    for (fault, expected) in cases() {
        let bytes = encode_fault_payload(&fault).unwrap();
        let value = graven_dynamic::read(&bytes, model).unwrap_or_else(|e| panic!("{fault:?}: {e}"));
        assert_eq!(value, expected, "{fault:?}");
    }
}

#[test]
fn schema_fault_bytes_decode_in_the_runtime() {
    let s = schema();
    let model = s.model(s.table("Fault").unwrap());
    for (fault, value) in cases() {
        let pool = graven::SimplePool::new(256);
        let mut b = graven::Builder::new(&pool);
        graven_dynamic::write(&value, model, &mut b).unwrap();
        let bytes = b.finished().unwrap().to_vec();
        assert_eq!(decode_fault(&bytes).unwrap(), fault);
    }
}
