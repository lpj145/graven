use graven::{Builder, Error, SimplePool, VerifyCtx};
use graven_schemas::bank::{Currency, Transaction, TransactionW};
use graven_schemas::game::{Frame, FrameW, Vec3};
use graven_schemas::iot::{Telemetry, TelemetryW};
use graven_schemas::stream::{Chunk, ChunkW};

type Ent = (u64, i64, Option<&'static str>);
const ENTRIES: [Ent; 3] = [(1, -500, Some("débito")), (2, 500, None), (3, 0, Some(""))];

fn fill_header(t: &mut TransactionW<'_>) {
    t.id(77).ts(1_700_000_000).currency(Currency::USD).from_shard(3).to_shard(9).note("pix");
}

fn check_bank(buf: &[u8]) {
    let t = Transaction::read(buf).unwrap();
    assert_eq!((t.id(), t.ts(), t.currency(), t.from_shard(), t.to_shard()), (77, 1_700_000_000, Currency::USD, 3, 9));
    assert_eq!(t.note().unwrap(), Some("pix"));
    let es = t.entries().unwrap();
    assert_eq!(es.len(), 3);
    for (e, want) in es.iter().zip(ENTRIES) {
        let e = e.unwrap();
        assert_eq!((e.account(), e.amount(), e.memo().unwrap()), want);
    }
    t.verify(&mut VerifyCtx::new(buf.len())).unwrap();
}

#[test]
fn bank_known_and_streaming_match() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        fill_header(&mut t);
        let mut l = t.entries(3);
        for (a, m, memo) in ENTRIES {
            let mut e = l.push();
            e.account(a).amount(m);
            if let Some(s) = memo {
                e.memo(s);
            }
        }
    }
    let known = b.finish().unwrap();
    check_bank(&known);

    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        let mut l = t.entries_streaming();
        for (a, m, memo) in ENTRIES {
            let mut e = l.push();
            e.account(a).amount(m);
            if let Some(s) = memo {
                e.memo(s);
            }
        }
        l.close();
        fill_header(&mut t);
    }
    let streamed = b.finish().unwrap();
    check_bank(&streamed);
}

#[test]
fn missing_required_is_err() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        t.id(1).ts(2).currency(Currency::BRL).from_shard(3);
        t.entries(0);
    }
    let buf = b.finish().unwrap();
    assert_eq!(Transaction::read(&buf).err(), Some(Error::Missing { field: "to_shard" }));
}

#[test]
fn missing_required_list_is_err() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    TransactionW::root(&mut b).id(1).ts(2).currency(Currency::BRL).from_shard(3).to_shard(4);
    let buf = b.finish().unwrap();
    assert_eq!(Transaction::read(&buf).err(), Some(Error::Missing { field: "entries" }));
}

#[test]
fn unknown_enum_value() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        t.id(1).ts(2).currency(Currency::Unknown(7)).from_shard(3).to_shard(4);
        t.entries(0);
    }
    let buf = b.finish().unwrap();
    assert_eq!(Transaction::read(&buf).unwrap().currency(), Currency::Unknown(7));
}

#[test]
fn game_roundtrip() {
    let pool = SimplePool::new(256);
    let mut b = Builder::new(&pool);
    {
        let mut f = FrameW::root(&mut b);
        f.tick(42);
        let mut l = f.entities(3);
        for i in 0..3u32 {
            l.push()
                .id(i)
                .pos(Vec3 { x: i as f32, y: 1.0, z: -1.0 })
                .vel(Vec3 { x: 0.5, y: 0.0, z: 0.25 })
                .hp(100 - i as u16)
                .alive(i != 1);
        }
    }
    let buf = b.finish().unwrap();
    let f = Frame::read(&buf).unwrap();
    assert_eq!(f.tick(), 42);
    let es = f.entities().unwrap();
    assert_eq!(es.len(), 3);
    let e1 = es.get(1).unwrap();
    assert_eq!((e1.id(), e1.hp(), e1.alive()), (1, 99, false));
    assert_eq!(e1.pos(), Vec3 { x: 1.0, y: 1.0, z: -1.0 });
    assert_eq!(e1.vel(), Vec3 { x: 0.5, y: 0.0, z: 0.25 });
    f.verify(&mut VerifyCtx::new(buf.len())).unwrap();
}

#[test]
fn iot_roundtrip_optional() {
    let pool = SimplePool::new(64);
    let mut b = Builder::new(&pool);
    TelemetryW::root(&mut b).device(5).ts(9).temp(21.5).battery(80);
    let buf = b.finish().unwrap();
    assert_eq!(buf.len(), 24);
    let t = Telemetry::read(&buf).unwrap();
    assert_eq!((t.device(), t.ts(), t.temp(), t.humidity(), t.battery()), (5, 9, 21.5, None, 80));
    let mut b = Builder::new(&pool);
    TelemetryW::root(&mut b).device(5).ts(9).temp(21.5).humidity(0.0).battery(80);
    let buf = b.finish().unwrap();
    assert_eq!(Telemetry::read(&buf).unwrap().humidity(), Some(0.0));
}

#[test]
fn stream_roundtrip() {
    let pool = SimplePool::new(64);
    let data: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
    let mut b = Builder::new(&pool);
    ChunkW::root(&mut b).stream_id(1).seq(2).data(&data);
    let buf = b.finish().unwrap();
    let c = Chunk::read(&buf).unwrap();
    assert_eq!((c.stream_id(), c.seq()), (1, 2));
    assert_eq!(c.data().unwrap(), &data[..]);
    c.verify(&mut VerifyCtx::new(buf.len())).unwrap();
}

#[test]
fn telemetry_truncated_optional_is_err() {
    let pool = SimplePool::new(64);
    let mut b = Builder::new(&pool);
    TelemetryW::root(&mut b).device(5).ts(9).temp(21.5).humidity(1.0).battery(80);
    let mut m = b.finish().unwrap().to_vec();
    // fixed_len 19 cuts humidity (17..21); battery (21) falls outside => also Missing, but Truncated wins
    m[0..2].copy_from_slice(&19u16.to_le_bytes());
    assert_eq!(Telemetry::read(&m).err(), Some(Error::Truncated));
}

/// Element type with no required fields: a list with elem_fixed_len=0 and a
/// huge count must hit the verify budget instead of looping ~4 billion times.
#[test]
fn zero_width_huge_list_hits_budget() {
    use graven::TableRef;
    use graven_schemas::{FromTable, ListR};
    struct Empty;
    impl<'a> FromTable<'a> for Empty {
        fn from_ref(_t: TableRef<'a>) -> Result<Self, Error> {
            Ok(Empty)
        }
        fn verify(&self, _ctx: &mut VerifyCtx) -> Result<(), Error> {
            Ok(())
        }
    }
    // [u32 count = u32::MAX][u16 efl = 0]
    let mut buf = u32::MAX.to_le_bytes().to_vec();
    buf.extend_from_slice(&0u16.to_le_bytes());
    let list = graven::TableList::at(&buf, 0).unwrap();
    let l: ListR<'_, Empty> = ListR::new(Some(list));
    let start = std::time::Instant::now();
    assert_eq!(l.verify(&mut VerifyCtx::new(buf.len())), Err(Error::BudgetExceeded));
    assert!(start.elapsed().as_millis() < 100);
}
