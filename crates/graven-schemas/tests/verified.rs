use bytes::BytesMut;
use proptest::prelude::*;
use graven::envelope::{encode, schema_id};
use graven::{Builder, Error, SimplePool};
use graven_schemas::bank::{self, Currency, Transaction, TransactionW};
use graven_schemas::{game, iot, stream};

type Ent = (u64, i64, Option<String>);

fn build_bank(id: u64, note: Option<&str>, entries: &[Ent]) -> Vec<u8> {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        t.id(id).ts(1_700_000_000).currency(Currency::EUR).from_shard(4).to_shard(8);
        if let Some(n) = note {
            t.note(n);
        }
        let mut l = t.entries(entries.len() as u32);
        for (a, m, memo) in entries {
            let mut e = l.push();
            e.account(*a).amount(*m);
            if let Some(s) = memo {
                e.memo(s);
            }
        }
    }
    b.finish().unwrap().to_vec()
}

fn wrap(id: u64, payload: &[u8]) -> BytesMut {
    let mut out = BytesMut::new();
    encode(id, payload, &mut out).unwrap();
    out
}

#[test]
fn schema_ids() {
    assert_eq!(bank::Transaction::SCHEMA_ID, schema_id("bank.Transaction"));
    assert_eq!(game::Frame::SCHEMA_ID, schema_id("game.Frame"));
    assert_eq!(iot::Telemetry::SCHEMA_ID, schema_id("iot.Telemetry"));
    assert_eq!(stream::Chunk::SCHEMA_ID, schema_id("stream.Chunk"));
    let ids = [bank::Transaction::SCHEMA_ID, game::Frame::SCHEMA_ID, iot::Telemetry::SCHEMA_ID, stream::Chunk::SCHEMA_ID];
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            assert_ne!(ids[i], ids[j]);
        }
    }
}

fn entry_strategy() -> impl Strategy<Value = Ent> {
    (any::<u64>(), any::<i64>(), proptest::option::of(any::<String>()))
}

proptest! {
    #[test]
    fn verified_reads_same_as_safe(
        id in any::<u64>(),
        note in proptest::option::of(any::<String>()),
        entries in proptest::collection::vec(entry_strategy(), 0..20),
    ) {
        let buf = build_bank(id, note.as_deref(), &entries);
        let safe = Transaction::read(&buf).unwrap();
        let v = Transaction::verified(&buf).unwrap();
        prop_assert_eq!(v.id(), safe.id());
        prop_assert_eq!(v.ts(), safe.ts());
        prop_assert_eq!(v.currency(), safe.currency());
        prop_assert_eq!(v.from_shard(), safe.from_shard());
        prop_assert_eq!(v.to_shard(), safe.to_shard());
        prop_assert_eq!(v.note(), safe.note().unwrap());
        prop_assert_eq!(v.note(), note.as_deref());
        let (ve, se) = (v.entries(), safe.entries().unwrap());
        prop_assert_eq!(ve.len(), se.len());
        prop_assert_eq!(ve.is_empty(), se.is_empty());
        prop_assert_eq!(ve.iter().len(), entries.len());
        for (i, (a, b)) in ve.iter().zip(se.iter()).enumerate() {
            let (a, b) = (a, b.unwrap());
            let g = ve.get(i).unwrap();
            prop_assert_eq!(a.account(), b.account());
            prop_assert_eq!(g.account(), b.account());
            prop_assert_eq!(a.amount(), b.amount());
            prop_assert_eq!(a.memo(), b.memo().unwrap());
            prop_assert_eq!(a.memo(), entries[i].2.as_deref());
        }
        prop_assert!(ve.get(entries.len()).is_none());
    }
}

#[test]
fn verified_rejects_bad_utf8() {
    let mut buf = build_bank(1, Some("xxNOTExx"), &[(1, 2, None)]);
    let pos = buf.windows(4).position(|w| w == b"NOTE").unwrap();
    buf[pos] = 0xFF;
    assert!(Transaction::read(&buf).is_ok());
    assert!(matches!(Transaction::verified(&buf), Err(Error::InvalidUtf8)));
}

#[test]
fn verified_rejects_bad_utf8_in_entry_memo() {
    let mut buf = build_bank(1, None, &[(1, 2, Some("xxMEMOxx".into()))]);
    let pos = buf.windows(4).position(|w| w == b"MEMO").unwrap();
    buf[pos] = 0xFF;
    assert!(matches!(Transaction::verified(&buf), Err(Error::InvalidUtf8)));
}

#[test]
fn verified_keeps_required_check() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        t.id(1).ts(2).currency(Currency::BRL).from_shard(3);
    }
    let buf = b.finish().unwrap().to_vec();
    assert!(matches!(Transaction::verified(&buf), Err(Error::Missing { .. })));
}

#[test]
fn from_bytes_roundtrip() {
    let payload = build_bank(4242, Some("pix"), &[(1, -5, Some("débito".into()))]);
    let env = wrap(bank::Transaction::SCHEMA_ID, &payload);
    let v = Transaction::from_bytes(&env).unwrap();
    assert_eq!(v.id(), 4242);
    assert_eq!(v.note(), Some("pix"));
    assert_eq!(v.entries().get(0).unwrap().memo(), Some("débito"));
}

#[test]
fn from_bytes_wrong_schema() {
    let payload = build_bank(1, None, &[]);
    let env = wrap(game::Frame::SCHEMA_ID, &payload);
    match Transaction::from_bytes(&env) {
        Err(Error::SchemaMismatch { expected, found }) => {
            assert_eq!(expected, bank::Transaction::SCHEMA_ID);
            assert_eq!(found, game::Frame::SCHEMA_ID);
        }
        Err(e) => panic!("unexpected error {e:?}"),
        Ok(_) => panic!("expected SchemaMismatch"),
    }
}

#[test]
fn from_bytes_empty_payload() {
    let env = wrap(bank::Transaction::SCHEMA_ID, b"");
    assert!(matches!(Transaction::from_bytes(&env), Err(Error::OutOfBounds)));
}

#[test]
fn from_bytes_bad_envelope_propagates() {
    let payload = build_bank(1, None, &[]);
    let mut env = wrap(bank::Transaction::SCHEMA_ID, &payload);
    let last = env.len() - 1;
    env[last] ^= 1;
    assert!(matches!(Transaction::from_bytes(&env), Err(Error::ChecksumMismatch)));
}

#[test]
fn other_roots_verified_and_from_bytes() {
    use game::{Frame, FrameW, Vec3};
    use iot::{Telemetry, TelemetryW};
    use stream::{Chunk, ChunkW};
    let pool = SimplePool::new(256);

    let mut b = Builder::new(&pool);
    {
        let mut f = FrameW::root(&mut b);
        f.tick(9);
        let mut l = f.entities(1);
        let v = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
        l.push().id(5).pos(v).vel(v).hp(10).alive(true);
    }
    let buf = b.finish().unwrap().to_vec();
    let fv = Frame::from_bytes(&wrap(game::Frame::SCHEMA_ID, &buf)).map(|f| f.tick()).unwrap();
    assert_eq!(fv, 9);
    let f = Frame::verified(&buf).unwrap();
    assert_eq!(f.entities().get(0).unwrap().id(), 5);
    assert!(matches!(Frame::from_bytes(&wrap(iot::Telemetry::SCHEMA_ID, &buf)), Err(Error::SchemaMismatch { .. })));

    let mut b = Builder::new(&pool);
    TelemetryW::root(&mut b).device(3).ts(4).temp(1.5).battery(80);
    let buf = b.finish().unwrap().to_vec();
    assert_eq!(Telemetry::verified(&buf).unwrap().device(), 3);
    assert_eq!(Telemetry::from_bytes(&wrap(iot::Telemetry::SCHEMA_ID, &buf)).unwrap().battery(), 80);
    assert!(matches!(Telemetry::from_bytes(&wrap(stream::Chunk::SCHEMA_ID, &buf)), Err(Error::SchemaMismatch { .. })));

    let mut b = Builder::new(&pool);
    ChunkW::root(&mut b).stream_id(7).seq(1).data(b"abc");
    let buf = b.finish().unwrap().to_vec();
    assert_eq!(Chunk::verified(&buf).unwrap().data(), b"abc");
    assert_eq!(Chunk::from_bytes(&wrap(stream::Chunk::SCHEMA_ID, &buf)).unwrap().seq(), 1);
    assert!(matches!(Chunk::from_bytes(&wrap(bank::Transaction::SCHEMA_ID, &buf)), Err(Error::SchemaMismatch { .. })));
}

// ------------------------------------------------- mutation acceptance gate

/// Deterministic xorshift64* PRNG (no extra dev-dependency, reproducible failures).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn string(&mut self) -> String {
        const CHARS: &[char] = &['a', 'Z', '0', ' ', 'é', 'ç', '€', '中', '𝄞', '\u{0}'];
        (0..self.below(12)).map(|_| CHARS[self.below(CHARS.len())]).collect()
    }
    fn opt_string(&mut self) -> Option<String> {
        (self.below(3) != 0).then(|| self.string())
    }
}

/// Every accessor of the verified view must equal the checked (safe) accessor.
fn assert_view_matches_checked(buf: &[u8], case: usize) {
    let Ok(v) = Transaction::verified(buf) else { return };
    let safe = Transaction::read(buf).expect("verified Ok ⇒ checked read Ok");
    assert_eq!(v.id(), safe.id(), "case {case}");
    assert_eq!(v.ts(), safe.ts(), "case {case}");
    assert_eq!(v.currency(), safe.currency(), "case {case}");
    assert_eq!(v.from_shard(), safe.from_shard(), "case {case}");
    assert_eq!(v.to_shard(), safe.to_shard(), "case {case}");
    let note = safe.note().expect("verified Ok ⇒ checked note Ok");
    assert_eq!(v.note(), note, "case {case}");
    let se = safe.entries().expect("verified Ok ⇒ checked entries Ok");
    let ve = v.entries();
    assert_eq!(ve.len(), se.len(), "case {case}");
    assert_eq!(ve.is_empty(), se.is_empty(), "case {case}");
    assert_eq!(ve.iter().len(), se.len(), "case {case}");
    for (i, a) in ve.iter().enumerate() {
        let g = ve.get(i).expect("verified Ok ⇒ verified entry Ok");
        let b = se.get(i).expect("verified Ok ⇒ checked entry Ok");
        assert_eq!(a.account(), b.account(), "case {case} entry {i}");
        assert_eq!(g.account(), b.account(), "case {case} entry {i}");
        assert_eq!(a.amount(), b.amount(), "case {case} entry {i}");
        assert_eq!(g.amount(), b.amount(), "case {case} entry {i}");
        let memo = b.memo().expect("verified Ok ⇒ checked memo Ok");
        assert_eq!(a.memo(), memo, "case {case} entry {i}");
        assert_eq!(g.memo(), memo, "case {case} entry {i}");
    }
    assert!(ve.get(se.len()).is_none(), "case {case}");
}

/// Acceptance gate for the codegen invariants (crate docs, "Invariants for the code
/// generator"): valid bank messages with 1–3 random byte mutations (and sometimes a
/// truncation); whenever `verified` accepts, the unchecked view must read exactly what the
/// checked API reads. Half of the cases only mutate bytes inside string payloads or scalar
/// slots, so a good share of them still verify Ok.
#[test]
fn verified_matches_checked_under_mutation() {
    const CASES: usize = if cfg!(miri) { 64 } else { 4096 };
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut ok, mut ok_mutated_only_noncritical, mut ok_any) = (0usize, 0usize, 0usize);
    for case in 0..CASES {
        let note = rng.opt_string();
        let entries: Vec<Ent> =
            (0..rng.below(6)).map(|_| (rng.next(), rng.next() as i64, rng.opt_string())).collect();
        let mut buf = build_bank(rng.next(), note.as_deref(), &entries);
        assert!(Transaction::verified(&buf).is_ok(), "case {case}: unmutated must verify");

        let noncritical = case % 2 == 0;
        // Candidate positions: for the "non-critical" half, the root's scalar slots (ids,
        // ts, currency, shards: bytes 3..27) plus string payload bytes; otherwise anywhere.
        let mut targets: Vec<usize> = Vec::new();
        if noncritical {
            targets.extend(3..27);
            for s in note.iter().chain(entries.iter().filter_map(|e| e.2.as_ref())) {
                if s.is_empty() {
                    continue;
                }
                if let Some(p) = buf.windows(s.len()).position(|w| w == s.as_bytes()) {
                    targets.extend(p..p + s.len());
                }
            }
        } else {
            targets.extend(0..buf.len());
        }
        for _ in 0..1 + rng.below(3) {
            let p = targets[rng.below(targets.len())];
            buf[p] = rng.next() as u8;
        }
        if !noncritical && rng.below(4) == 0 {
            let n = rng.below(buf.len() + 1);
            buf.truncate(n);
        }

        if Transaction::verified(&buf).is_ok() {
            ok += 1;
            if noncritical {
                ok_mutated_only_noncritical += 1;
            } else {
                ok_any += 1;
            }
        }
        assert_view_matches_checked(&buf, case);
    }
    eprintln!(
        "mutation gate: {CASES} cases, verified Ok = {ok} \
         (non-critical half: {ok_mutated_only_noncritical}, anywhere half: {ok_any})"
    );
    assert!(ok_mutated_only_noncritical > CASES / 8, "too few non-critical mutants verified Ok: {ok}");
    assert!(ok_any > 0, "no anywhere-mutant verified Ok");
}

// ------------------------------------------------ frozen generable pattern

#[test]
fn frame_verified_entities_are_views() {
    use game::{EntityVerified, Frame, FrameW, Vec3};
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut f = FrameW::root(&mut b);
        f.tick(3);
        let mut l = f.entities(2);
        let v = Vec3 { x: 1.0, y: -2.0, z: 0.5 };
        l.push().id(11).pos(v).vel(v).hp(7).alive(true);
        l.push().id(12).pos(v).vel(v).hp(8).alive(false);
    }
    let buf = b.finish().unwrap().to_vec();
    let checked = Frame::read(&buf).unwrap();
    let fv = Frame::verified(&buf).unwrap();
    assert_eq!(fv.tick(), checked.tick());
    let ve = fv.entities();
    let ce = checked.entities().unwrap();
    assert_eq!(ve.len(), ce.len());
    for i in 0..ve.len() {
        let e: EntityVerified<'_> = ve.get(i).unwrap();
        let c = ce.get(i).unwrap();
        assert_eq!(e.id(), c.id());
        assert_eq!(e.pos(), c.pos());
        assert_eq!(e.vel(), c.vel());
        assert_eq!(e.hp(), c.hp());
        assert_eq!(e.alive(), c.alive());
        assert_eq!(e.reader().id(), c.id());
    }
    assert_eq!(fv.reader().tick(), 3);
}

#[test]
fn to_bytes_roundtrip_typed() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new_enveloped(&pool);
    let env = TransactionW::to_bytes(&mut b, |t| {
        t.id(9).ts(10).currency(Currency::USD).from_shard(1).to_shard(2).note("typed");
        t.entries(1).push().account(5).amount(-6).memo("m");
    })
    .unwrap();
    let v = Transaction::from_bytes(&env).unwrap();
    assert_eq!(v.id(), 9);
    assert_eq!(v.note(), Some("typed"));
    assert_eq!(v.entries().get(0).unwrap().memo(), Some("m"));
    assert_eq!(graven::envelope::decode(&env).unwrap().schema_id, Transaction::SCHEMA_ID);
    assert_eq!(Transaction::SCHEMA_ID, bank::Transaction::SCHEMA_ID);

    // `seal` batches envelopes; `take_frames` hands them out concatenated.
    for id in [1u64, 2] {
        TransactionW::seal(&mut b, |t| {
            t.id(id).ts(0).currency(Currency::BRL).from_shard(0).to_shard(0);
            t.entries(0);
        })
        .unwrap();
    }
    let batch = b.take_frames();
    let ids: Vec<u64> = graven::envelope::envelopes(&batch)
        .map(|e| {
            let e = e.unwrap();
            assert_eq!(e.schema_id, Transaction::SCHEMA_ID);
            Transaction::verified(e.payload).unwrap().id()
        })
        .collect();
    assert_eq!(ids, [1, 2]);
}

#[test]
fn every_table_is_root_capable() {
    use bank::{Entry, EntryW};
    use game::{Entity, EntityW, Frame, FrameW, Vec3};
    use iot::{Telemetry, TelemetryW};
    use stream::{Chunk, ChunkW};
    use graven_schemas::evo::{bank_v2, bank_v3};
    let pool = SimplePool::new(256);
    let mut b = Builder::new_enveloped(&pool);

    assert_eq!(Entry::SCHEMA_ID, schema_id("bank.Entry"));
    assert_eq!(Entity::SCHEMA_ID, schema_id("game.Entity"));
    assert_eq!(Frame::SCHEMA_ID, game::Frame::SCHEMA_ID);
    assert_eq!(Telemetry::SCHEMA_ID, iot::Telemetry::SCHEMA_ID);
    assert_eq!(Chunk::SCHEMA_ID, stream::Chunk::SCHEMA_ID);
    assert_eq!(bank_v2::Transaction::SCHEMA_ID, bank::Transaction::SCHEMA_ID);
    assert_eq!(bank_v3::Transaction::SCHEMA_ID, bank::Transaction::SCHEMA_ID);

    let env = EntryW::to_bytes(&mut b, |e| {
        e.account(1).amount(2).memo("só");
    })
    .unwrap();
    let e = Entry::from_bytes(&env).unwrap();
    assert_eq!((e.account(), e.amount(), e.memo()), (1, 2, Some("só")));
    assert!(matches!(Transaction::from_bytes(&env), Err(Error::SchemaMismatch { .. })));

    let v = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
    let env = EntityW::to_bytes(&mut b, |e| {
        e.id(4).pos(v).vel(v).hp(5).alive(true);
    })
    .unwrap();
    assert_eq!(Entity::from_bytes(&env).unwrap().hp(), 5);
    assert_eq!(Entity::read(&env[20..env.len() - 4]).unwrap().id(), 4);

    let env = FrameW::to_bytes(&mut b, |f| {
        f.tick(8).entities(0);
    })
    .unwrap();
    assert_eq!(Frame::from_bytes(&env).unwrap().tick(), 8);

    let env = TelemetryW::to_bytes(&mut b, |t| {
        t.device(1).ts(2).temp(3.0).battery(4);
    })
    .unwrap();
    assert_eq!(Telemetry::from_bytes(&env).unwrap().humidity(), None);

    let env = ChunkW::to_bytes(&mut b, |c| {
        c.stream_id(1).seq(2).data(b"xy");
    })
    .unwrap();
    assert_eq!(Chunk::from_bytes(&env).unwrap().data(), b"xy");

    let env = bank_v2::TransactionW::to_bytes(&mut b, |t| {
        t.id(1).ts(2).currency(bank_v2::Currency::EUR).to_shard(3).fee(-4);
        t.entries(1).push().account(5).amount(6).memo("v2");
    })
    .unwrap();
    let t2 = bank_v2::Transaction::from_bytes(&env).unwrap();
    assert_eq!((t2.id(), t2.ts(), t2.fee(), t2.from_shard()), (1, 2, Some(-4), None));
    assert_eq!(t2.entries().get(0).unwrap().memo(), Some("v2"));
    let t3 = bank_v3::Transaction::from_bytes(&env).unwrap();
    assert_eq!((t3.id(), t3.fee()), (1, Some(-4)));
    assert_eq!(t3.entries().get(0).unwrap().memo(), Some("v2"));
    assert_eq!(t3.reader().currency(), bank_v3::Currency::EUR);

    bank_v3::TransactionW::seal(&mut b, |t| {
        t.id(1).ts(2).currency(bank_v3::Currency::EUR).entries(0);
    })
    .unwrap();
    assert_eq!(graven::envelope::envelopes(&b.take_frames()).count(), 1);
}

/// Header charging moved from `T::verify` to `verified` (the root) and to the parent
/// (nested tables); the totals must not change. Numbers measured with the code *before*
/// the move: `Transaction::read(buf).verify(ctx)` / `Frame::read(buf).verify(ctx)` with
/// `VerifyCtx::new(buf.len())` (1 MiB budget).
#[test]
fn budget_totals_unchanged() {
    use game::{Frame, FrameW, Vec3};
    use graven::VerifyCtx;
    let pool = SimplePool::new(256);

    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        t.id(42).ts(1_700_000_000).currency(Currency::EUR).from_shard(4).to_shard(8).note("pix");
        let mut l = t.entries(2);
        l.push().account(1).amount(-5).memo("débito");
        l.push().account(2).amount(7);
    }
    let buf = b.finish().unwrap().to_vec();
    assert_eq!(buf.len(), 103);
    let mut ctx = VerifyCtx::new(buf.len());
    Transaction::verified_with(&buf, &mut ctx).unwrap();
    assert_eq!(ctx.budget(), 1_048_473);

    let mut b = Builder::new(&pool);
    {
        let mut f = FrameW::root(&mut b);
        f.tick(9);
        let mut l = f.entities(3);
        let v = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
        for i in 0..3 {
            l.push().id(i).pos(v).vel(v).hp(10).alive(i % 2 == 0);
        }
    }
    let buf = b.finish().unwrap().to_vec();
    assert_eq!(buf.len(), 114);
    let mut ctx = VerifyCtx::new(buf.len());
    Frame::verified_with(&buf, &mut ctx).unwrap();
    assert_eq!(ctx.budget(), 1_048_462);

    // A budget one byte short of the total fails, the exact total passes.
    let mut b = Builder::new(&pool);
    TelemetryRoot::build(&mut b);
    let buf = b.finish().unwrap().to_vec();
    let total = 2 + iot::telemetry::FIXED_LEN as usize;
    assert!(iot::Telemetry::verified_with(&buf, &mut VerifyCtx::with_limits(64, total)).is_ok());
    assert_eq!(
        iot::Telemetry::verified_with(&buf, &mut VerifyCtx::with_limits(64, total - 1)).err(),
        Some(Error::BudgetExceeded)
    );
}

struct TelemetryRoot;
impl TelemetryRoot {
    fn build(b: &mut Builder<'_>) {
        iot::TelemetryW::root(b).device(1).ts(2).temp(3.0).battery(4);
    }
}

/// The public inherent `X::verify` is the root entry point: it charges the root header, so
/// `X::read(buf)?.verify(ctx)` leaves exactly the budget `X::verified_with` leaves.
#[test]
fn direct_verify_matches_verified_with() {
    use game::{Frame, FrameW, Vec3};
    use graven::VerifyCtx;
    let pool = SimplePool::new(256);
    let bank = build_bank(42, Some("pix"), &[(1, -5, Some("débito".into())), (2, 7, None)]);

    let mut b = Builder::new(&pool);
    {
        let mut f = FrameW::root(&mut b);
        f.tick(9);
        let mut l = f.entities(3);
        let v = Vec3 { x: 1.0, y: 2.0, z: 3.0 };
        for i in 0..3 {
            l.push().id(i).pos(v).vel(v).hp(10).alive(i % 2 == 0);
        }
    }
    let game = b.finish().unwrap().to_vec();

    let mut b = Builder::new(&pool);
    iot::TelemetryW::root(&mut b).device(1).ts(2).temp(3.0).battery(4);
    let tele = b.finish().unwrap().to_vec();
    let mut b = Builder::new(&pool);
    stream::ChunkW::root(&mut b).stream_id(1).seq(2).data(b"abc");
    let chunk = b.finish().unwrap().to_vec();

    macro_rules! same {
        ($t:ty, $buf:expr) => {{
            let buf: &[u8] = &$buf;
            let mut a = VerifyCtx::new(buf.len());
            <$t>::read(buf).unwrap().verify(&mut a).unwrap();
            let mut v = VerifyCtx::new(buf.len());
            <$t>::verified_with(buf, &mut v).unwrap();
            assert_eq!(a.budget(), v.budget(), stringify!($t));
            a.budget()
        }};
    }
    assert_eq!(same!(Transaction<'_>, bank), 1_048_473);
    assert_eq!(same!(Frame<'_>, game), 1_048_462);
    same!(iot::Telemetry<'_>, tele);
    same!(stream::Chunk<'_>, chunk);
    same!(graven_schemas::evo::bank_v2::Transaction<'_>, bank);
}
