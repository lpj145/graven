//! The data tools of `gravenc`: `from-json`, `inspect`, `to-json`, `scan` and `diff`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tools-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn gravenc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gravenc")).args(args).output().unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const SCHEMA: &str = "namespace shop;
enum Status : u8 { Open = 0, Paid = 1 }
struct Money { cents: i64, currency: u16 }
table Item { sku @0: string; qty @1: u32 = 1; }
table Note { text @0: string; }
union Extra { Note @1: Note }
table Order {
  id     @0: u64;
  status @1: Status = Open;
  total  @2: Money;
  items  @3: [Item];
  tags   @4: [string]?;
  blob   @5: bytes?;
  extra  @6: Extra?;
  ratio  @7: f64?;
}
";

const ORDERS: &str = r#"{"id": 1, "status": "Paid", "total": {"cents": -1250, "currency": 986}, "items": [{"sku": "A-1", "qty": 3}, {"sku": "B-2"}], "tags": ["x", ""], "blob": "00ff", "extra": {"Note": {"text": "gift"}}, "ratio": 0.5}
{"id": 2, "total": {"cents": 0, "currency": 840}, "items": []}
"#;

/// A schema with its lock, and `orders.bin` written from `ORDERS` by `from-json`.
fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let d = tmp(name);
    let schema = d.join("shop.graven");
    std::fs::write(&schema, SCHEMA).unwrap();
    assert_eq!(gravenc(&["update", s(&schema), "--init"]).status.code(), Some(0));
    let json = d.join("orders.jsonl");
    std::fs::write(&json, ORDERS).unwrap();
    let bin = d.join("orders.bin");
    let o = gravenc(&["from-json", s(&schema), "--root", "Order", s(&json), "-o", s(&bin)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    (d, schema, bin)
}

#[test]
fn json_round_trip_through_envelopes() {
    let (_d, schema, bin) = setup("roundtrip");
    let o = gravenc(&["to-json", s(&schema), s(&bin)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let lines: Vec<serde_json::Value> = out(&o).lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    // Defaults are shown when the field was not written (`qty` of B-2, `status` of order 2).
    let want: Vec<serde_json::Value> = ORDERS.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let mut first = want[0].clone();
    assert_eq!(lines[0]["items"][1]["qty"], 1);
    first["items"][1]["qty"] = 1.into();
    assert_eq!(lines[0], first);
    assert_eq!(lines[1]["status"], "Open");
    // Re-encoding the decoded JSON gives the same bytes.
    let d2 = bin.with_file_name("again.jsonl");
    std::fs::write(&d2, out(&o)).unwrap();
    let bin2 = bin.with_file_name("again.bin");
    assert_eq!(gravenc(&["from-json", s(&schema), "--root", "Order", s(&d2), "-o", s(&bin2)]).status.code(), Some(0));
    let o2 = gravenc(&["to-json", s(&schema), s(&bin2)]);
    assert_eq!(out(&o2), out(&o));
}

#[test]
fn raw_message_needs_root() {
    let (d, schema, _) = setup("raw");
    let one = d.join("one.json");
    std::fs::write(&one, ORDERS.lines().next().unwrap()).unwrap();
    let raw = d.join("one.bin");
    assert_eq!(gravenc(&["from-json", s(&schema), "--root", "Order", "--raw", s(&one), "-o", s(&raw)]).status.code(), Some(0));
    let o = gravenc(&["inspect", s(&schema), s(&raw)]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("--root"), "{}", err(&o));
    let o = gravenc(&["inspect", s(&schema), s(&raw), "--root", "Order"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("\"sku\": \"A-1\""), "{}", out(&o));
}

#[test]
fn inspect_names_each_record() {
    let (_d, schema, bin) = setup("inspect");
    let o = gravenc(&["inspect", s(&schema), s(&bin)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = out(&o);
    assert!(text.starts_with("#0  offset 0  shop.Order  "), "{text}");
    assert!(text.contains("#1  offset "), "{text}");
    assert!(text.contains("\"Note\": {"), "{text}");
}

#[test]
fn from_json_rejects_bad_documents() {
    let (d, schema, _) = setup("badjson");
    for (doc, needle) in [
        (r#"{"id": 1, "items": []}"#, "required field `total` is missing"),
        (r#"{"id": 1, "total": {"cents": 1, "currency": 1}, "items": [], "nope": 1}"#, "has no field `nope`"),
        (r#"{"id": -1, "total": {"cents": 1, "currency": 1}, "items": []}"#, "does not fit in u64"),
        (r#"{"id": 1, "status": "Lost", "total": {"cents": 1, "currency": 1}, "items": []}"#, "`Lost` is not a variant"),
        (r#"{"id": 1, "total": {"cents": 1}, "items": []}"#, "`currency` is missing"),
        (r#"{"id": 1, "total": null, "items": []}"#, "required field `total` is missing"),
    ] {
        let f = d.join("bad.json");
        std::fs::write(&f, doc).unwrap();
        let o = gravenc(&["from-json", s(&schema), "--root", "Order", s(&f), "-o", s(&d.join("x.bin"))]);
        assert_eq!(o.status.code(), Some(1), "{doc}");
        assert!(err(&o).contains(needle), "{doc}: {}", err(&o));
    }
}

#[test]
fn scan_reports_and_truncates_an_incomplete_tail() {
    let (_d, schema, bin) = setup("scan");
    let full = std::fs::read(&bin).unwrap();
    let o = gravenc(&["scan", s(&bin), "--schema", s(&schema)]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("2 record(s) intact"), "{}", out(&o));
    assert!(out(&o).contains("shop.Order"), "{}", out(&o));

    // A crash mid-write: the last record is cut.
    std::fs::write(&bin, &full[..full.len() - 7]).unwrap();
    let o = gravenc(&["scan", s(&bin)]);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).contains("incomplete tail"), "{}", out(&o));
    let o = gravenc(&["scan", s(&bin), "--truncate"]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let first_len = std::fs::read(&bin).unwrap().len();
    assert!(first_len < full.len() - 7);
    assert_eq!(gravenc(&["scan", s(&bin)]).status.code(), Some(0));
    assert_eq!(&std::fs::read(&bin).unwrap()[..], &full[..first_len]);
}

#[test]
fn scan_never_truncates_corruption() {
    let (_d, _schema, bin) = setup("corrupt");
    let mut data = std::fs::read(&bin).unwrap();
    data[25] ^= 0x01; // inside the first record's payload
    std::fs::write(&bin, &data).unwrap();
    let o = gravenc(&["scan", s(&bin), "--truncate"]);
    assert_eq!(o.status.code(), Some(1));
    let text = out(&o);
    assert!(text.contains("corrupt data at offset 0: ChecksumMismatch"), "{text}");
    assert!(text.contains("1 intact record(s) follow"), "{text}");
    assert_eq!(std::fs::read(&bin).unwrap(), data, "corrupt data must not be truncated");
}

#[test]
fn diff_lists_changes_with_verdicts() {
    let (d, schema, _) = setup("diff");
    let o = gravenc(&["diff", s(&schema)]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("no changes"), "{}", out(&o));

    let next = SCHEMA
        .replace("  ratio  @7: f64?;\n", "  ratio  @7: f64?;\n  note   @8: string?;\n")
        .replace("Paid = 1 }", "Paid = 1, Void = 2 }");
    std::fs::write(&schema, &next).unwrap();
    let o = gravenc(&["diff", s(&schema)]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let text = out(&o);
    assert!(text.contains("+ note @8: string?") && text.contains("allowed: new-optional"), "{text}");
    assert!(text.contains("+ Void = 2") && text.contains("allowed: new-enum-variant"), "{text}");

    std::fs::write(&schema, next.replace("id     @0: u64;", "id     @0: u32;")).unwrap();
    let o = gravenc(&["diff", s(&schema)]);
    assert_eq!(o.status.code(), Some(1));
    let text = out(&o);
    assert!(text.contains("~ id @0: type u64 → u32") && text.contains("error: type-changed"), "{text}");
    // Fields moved only by that size change are not listed as changes of their own.
    assert!(!text.contains("layout changed"), "{text}");
    let _ = d;
}

#[test]
fn from_json_reads_a_top_level_array_as_one_message_per_element() {
    let (d, schema, bin) = setup("array");
    let docs: Vec<serde_json::Value> = ORDERS.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let arr = d.join("orders.json");
    std::fs::write(&arr, serde_json::to_string_pretty(&docs).unwrap()).unwrap();
    let bin2 = d.join("from-array.bin");
    let o = gravenc(&["from-json", s(&schema), "--root", "Order", s(&arr), "-o", s(&bin2)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(std::fs::read(&bin2).unwrap(), std::fs::read(&bin).unwrap(), "same bytes as the JSON Lines input");
}

#[test]
fn from_json_treats_null_as_absent() {
    let (d, schema, _) = setup("null");
    let f = d.join("n.json");
    std::fs::write(&f, r#"{"id": 9, "status": null, "total": {"cents": 1, "currency": 1}, "items": [], "tags": null}"#).unwrap();
    let bin = d.join("n.bin");
    assert_eq!(gravenc(&["from-json", s(&schema), "--root", "Order", s(&f), "-o", s(&bin)]).status.code(), Some(0));
    let o = gravenc(&["to-json", s(&schema), s(&bin)]);
    let v: serde_json::Value = serde_json::from_str(out(&o).trim()).unwrap();
    assert_eq!(v["status"], "Open", "absent field with a default reads the default");
    assert!(v.get("tags").is_none(), "absent optional field is left out");
}

#[test]
fn dynamic_reader_applies_extreme_defaults() {
    let d = tmp("defaults");
    let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("../graven-codegen-tests/schemas/defaults.graven");
    let one = d.join("one.json");
    std::fs::write(&one, r#"{"id": 1}"#).unwrap();
    let bin = d.join("one.bin");
    let o = gravenc(&["from-json", s(&schema), "--root", "Extremes", "--raw", s(&one), "-o", s(&bin)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let o = gravenc(&["to-json", s(&schema), s(&bin), "--root", "Extremes"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(out(&o).trim()).unwrap();
    assert_eq!(v["u64_max"], serde_json::json!(u64::MAX));
    assert_eq!(v["u128_max"], u128::MAX.to_string());
    assert_eq!(v["u128_mid"], (1u128 << 127).to_string());
    assert_eq!(v["i64_min"], serde_json::json!(i64::MIN));
    assert_eq!(v["i128_min"], i128::MIN.to_string());
    assert_eq!(v["f32_neg0"].as_f64().unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(v["f64_tiny"].as_f64().unwrap().to_bits(), 1);
    assert_eq!(v["big"], "High");
}
