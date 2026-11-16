//! What the reference implementation reads from arbitrary bytes, for differential testing of
//! other implementations (`ts/tests/fuzz.ts`).
//!
//! Reads one case per line on stdin and writes one JSON line per case on stdout:
//!
//! - `format <schema> <0|1> <hex>`: reads `<hex>` with the reader schema `<schema>`
//!   (`1` = enveloped) through the conformance interpreter, and writes
//!   `{"value": …}` or `{"error": "<Error variant>"}`, the `expect` of a format v1 vector.
//! - `rpc <hex>`: decodes every frame in `<hex>`, and writes `{"frames": […]}` or
//!   `{"error": "…"}`, the `.json` of an RPC frame vector.

use std::io::{self, BufRead, BufWriter, Write};

use graven_conformance::rpc_vectors::decode_expect;
use graven_conformance::vectors::{ExpectJson, error_name, evaluate, reader_schema};

fn hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex digit")).collect()
}

fn format_case(schema: &str, enveloped: bool, bytes: &[u8]) -> String {
    assert!(reader_schema(schema).is_some(), "unknown reader schema `{schema}`");
    let expect = match evaluate(bytes, schema, enveloped) {
        Ok(v) => ExpectJson::Value(v),
        Err(e) => ExpectJson::Error(error_name(&e)),
    };
    serde_json::to_string(&expect).expect("serializes")
}

fn rpc_case(bytes: &[u8]) -> String {
    serde_json::to_string(&decode_expect(bytes)).expect("serializes")
}

fn main() {
    let stdin = io::stdin();
    let mut out = BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line.expect("read stdin");
        let parts: Vec<&str> = line.split(' ').collect();
        let result = match parts.as_slice() {
            ["format", schema, env, h] => format_case(schema, *env == "1", &hex(h)),
            ["rpc", h] => rpc_case(&hex(h)),
            ["rpc"] => rpc_case(&[]),
            ["format", schema, env] => format_case(schema, *env == "1", &[]),
            _ => panic!("bad input line: {line}"),
        };
        writeln!(out, "{result}").expect("write stdout");
    }
}
