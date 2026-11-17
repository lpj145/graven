//! Evolution matrix of `bank` (spec §4.2, §9), with the generated code of the three versions:
//!
//! - v1 = `schemas/bank.graven`;
//! - v2 = `schemas/evo/bank_v2.graven` (`from_shard` optional, `note` reserved, `fee` new);
//! - v3 = `schemas/evo/bank_v3.graven` (v2 + `to_shard` reserved).
//!
//! Each writer is read by each reader along three paths (`read` + checked accessors,
//! `verified` + view, envelope with `from_bytes`) and all three must give the same cell.

use graven::{Builder, Error, SimplePool};
use graven_schemas::bank as v1;
use graven_schemas::evo::{bank_v2 as v2, bank_v3 as v3};

/// What a reader sees. A field the reader's version does not have (or has reserved) is `None`.
#[derive(Debug, Clone, PartialEq)]
struct View {
    id: u64,
    ts: u64,
    currency: u16,
    from_shard: Option<u32>,
    to_shard: Option<u32>,
    entries: Vec<(u64, i64, Option<String>)>,
    note: Option<String>,
    fee: Option<i64>,
}

const ENTRIES: [(u64, i64, &str); 2] = [(10, -1, "m"), (11, 2, "")];

fn entries_value() -> Vec<(u64, i64, Option<String>)> {
    ENTRIES.iter().map(|&(a, m, s)| (a, m, if s.is_empty() { None } else { Some(s.to_string()) })).collect()
}

/// Writers: `(name, raw message, enveloped message)`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Writer {
    /// v1 with every field, `note = "n"`.
    V1,
    /// v2 with `from_shard = 3`, `to_shard = 4`, `fee = -5`.
    V2,
    /// v2 without `from_shard`.
    V2NoFrom,
    /// v3 (`to_shard` no longer exists), `fee = -5`.
    V3,
}

macro_rules! fill_entries {
    ($t:expr) => {{
        let mut l = $t.entries(ENTRIES.len() as u32);
        for &(a, m, s) in &ENTRIES {
            let mut e = l.push();
            e.account(a).amount(m);
            if !s.is_empty() {
                e.memo(s);
            }
        }
    }};
}

fn write(w: Writer, enveloped: bool) -> Vec<u8> {
    let pool = SimplePool::new(1024);
    let mut b = if enveloped { Builder::new_enveloped(&pool) } else { Builder::new(&pool) };
    macro_rules! go {
        ($m:ident, |$t:ident| $body:block) => {{
            if enveloped {
                $m::TransactionW::to_bytes(&mut b, |$t| $body).unwrap().to_vec()
            } else {
                {
                    let mut tw = $m::TransactionW::root(&mut b);
                    let $t = &mut tw;
                    $body
                }
                b.finish().unwrap().to_vec()
            }
        }};
    }
    match w {
        Writer::V1 => go!(v1, |t| {
            t.id(1).ts(2).currency(v1::Currency::EUR).from_shard(3).to_shard(4).note("n");
            fill_entries!(t);
        }),
        Writer::V2 => go!(v2, |t| {
            t.id(1).ts(2).currency(v2::Currency::EUR).from_shard(3).to_shard(4).fee(-5);
            fill_entries!(t);
        }),
        Writer::V2NoFrom => go!(v2, |t| {
            t.id(1).ts(2).currency(v2::Currency::EUR).to_shard(4).fee(-5);
            fill_entries!(t);
        }),
        Writer::V3 => go!(v3, |t| {
            t.id(1).ts(2).currency(v3::Currency::EUR).from_shard(3).fee(-5);
            fill_entries!(t);
        }),
    }
}

/// A checked reader returns `Result`s, a verified view the values themselves; `res()` turns
/// both into a `Result` so the macros below read either the same way.
trait Res {
    type T;
    fn res(self) -> Result<Self::T, Error>;
}
impl<T> Res for Result<T, Error> {
    type T = T;
    fn res(self) -> Result<T, Error> {
        self
    }
}
impl<T> Res for Option<T> {
    type T = Option<T>;
    fn res(self) -> Result<Option<T>, Error> {
        Ok(self)
    }
}
impl<'a, T> Res for graven::rt::VListR<'a, T> {
    type T = Self;
    fn res(self) -> Result<Self, Error> {
        Ok(self)
    }
}
macro_rules! res_identity {
    ($($t:ty),*) => {$(impl Res for $t { type T = Self; fn res(self) -> Result<Self, Error> { Ok(self) } })*};
}
res_identity!(v1::EntryVerified<'_>, v2::EntryVerified<'_>, v3::EntryVerified<'_>);

/// Entries read by any version (checked or verified: same names).
macro_rules! entries_of {
    ($t:expr) => {{
        let mut out = Vec::new();
        for e in $t.entries().res()?.iter() {
            let e = e.res()?;
            out.push((e.account(), e.amount(), e.memo().res()?.map(str::to_string)));
        }
        out
    }};
}

macro_rules! view_v1 {
    ($t:expr) => {{
        let t = $t;
        View {
            id: t.id(),
            ts: t.ts(),
            currency: t.currency().to_raw(),
            from_shard: Some(t.from_shard()),
            to_shard: Some(t.to_shard()),
            entries: entries_of!(t),
            note: t.note().res()?.map(str::to_string),
            fee: None,
        }
    }};
}

macro_rules! view_v2 {
    ($t:expr) => {{
        let t = $t;
        View {
            id: t.id(),
            ts: t.ts(),
            currency: t.currency().to_raw(),
            from_shard: t.from_shard(),
            to_shard: Some(t.to_shard()),
            entries: entries_of!(t),
            note: None,
            fee: t.fee(),
        }
    }};
}

macro_rules! view_v3 {
    ($t:expr) => {{
        let t = $t;
        View {
            id: t.id(),
            ts: t.ts(),
            currency: t.currency().to_raw(),
            from_shard: t.from_shard(),
            to_shard: None,
            entries: entries_of!(t),
            note: None,
            fee: t.fee(),
        }
    }};
}

#[derive(Clone, Copy, Debug)]
enum Reader {
    V1,
    V2,
    V3,
}

#[derive(Clone, Copy, Debug)]
enum Path {
    Read,
    Verified,
    Envelope,
}

fn read(r: Reader, p: Path, b: &[u8]) -> Result<View, Error> {
    Ok(match (r, p) {
        (Reader::V1, Path::Read) => view_v1!(v1::Transaction::read(b)?),
        (Reader::V1, Path::Verified) => view_v1!(v1::Transaction::verified(b)?),
        (Reader::V1, Path::Envelope) => view_v1!(v1::Transaction::from_bytes(b)?),
        (Reader::V2, Path::Read) => view_v2!(v2::Transaction::read(b)?),
        (Reader::V2, Path::Verified) => view_v2!(v2::Transaction::verified(b)?),
        (Reader::V2, Path::Envelope) => view_v2!(v2::Transaction::from_bytes(b)?),
        (Reader::V3, Path::Read) => view_v3!(v3::Transaction::read(b)?),
        (Reader::V3, Path::Verified) => view_v3!(v3::Transaction::verified(b)?),
        (Reader::V3, Path::Envelope) => view_v3!(v3::Transaction::from_bytes(b)?),
    })
}

fn base(from_shard: Option<u32>, to_shard: Option<u32>, note: Option<&str>, fee: Option<i64>) -> View {
    View {
        id: 1,
        ts: 2,
        currency: 2,
        from_shard,
        to_shard,
        entries: entries_value(),
        note: note.map(str::to_string),
        fee,
    }
}

/// The expected matrix, cell by cell.
fn expected(w: Writer, r: Reader) -> Result<View, Error> {
    use Reader as R;
    use Writer as W;
    match (w, r) {
        (W::V1, R::V1) => Ok(base(Some(3), Some(4), Some("n"), None)),
        (W::V1, R::V2) => Ok(base(Some(3), Some(4), None, None)),
        (W::V1, R::V3) => Ok(base(Some(3), None, None, None)),
        (W::V2, R::V1) => Ok(base(Some(3), Some(4), None, None)),
        (W::V2, R::V2) => Ok(base(Some(3), Some(4), None, Some(-5))),
        (W::V2, R::V3) => Ok(base(Some(3), None, None, Some(-5))),
        (W::V2NoFrom, R::V1) => Err(Error::Missing { field: "from_shard" }),
        (W::V2NoFrom, R::V2) => Ok(base(None, Some(4), None, Some(-5))),
        (W::V2NoFrom, R::V3) => Ok(base(None, None, None, Some(-5))),
        (W::V3, R::V1) => Err(Error::Missing { field: "to_shard" }),
        (W::V3, R::V2) => Err(Error::Missing { field: "to_shard" }),
        (W::V3, R::V3) => Ok(base(Some(3), None, None, Some(-5))),
    }
}

const WRITERS: [Writer; 4] = [Writer::V1, Writer::V2, Writer::V2NoFrom, Writer::V3];
const READERS: [Reader; 3] = [Reader::V1, Reader::V2, Reader::V3];

#[test]
fn evolution_matrix() {
    for w in WRITERS {
        let raw = write(w, false);
        for r in READERS {
            let want = expected(w, r);
            for p in [Path::Read, Path::Verified] {
                assert_eq!(read(r, p, &raw), want, "writer {w:?} → reader {r:?} via {p:?}");
            }
        }
    }
}

/// Same namespace and same table name ⇒ same `SCHEMA_ID`: one version's envelope is
/// accepted by another's `from_bytes`, and the result follows the matrix.
#[test]
fn envelope_cross_versions() {
    assert_eq!(v1::Transaction::SCHEMA_ID, v2::Transaction::SCHEMA_ID);
    assert_eq!(v2::Transaction::SCHEMA_ID, v3::Transaction::SCHEMA_ID);
    for w in WRITERS {
        let env = write(w, true);
        for r in READERS {
            assert_eq!(read(r, Path::Envelope, &env), expected(w, r), "envelope {w:?} → {r:?}");
        }
    }
}
