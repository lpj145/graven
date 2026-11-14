use graven::{Builder, Error, SimplePool};
use graven_layout::{FieldKind::*, compute_layout};
use graven_schemas::bank::{Currency, Transaction as V1, TransactionW as V1W};
use graven_schemas::evo::{bank_v2, bank_v3};

fn v1_msg(pool: &SimplePool, from_shard: u32) -> bytes::Bytes {
    let mut b = Builder::new(pool);
    {
        let mut t = V1W::root(&mut b);
        t.id(1).ts(2).currency(Currency::EUR).from_shard(from_shard).to_shard(4).note("antiga");
        t.entries(1).push().account(10).amount(-10);
    }
    b.finish().unwrap()
}

fn v2_msg(pool: &SimplePool) -> bytes::Bytes {
    let mut b = Builder::new(pool);
    {
        let mut t = bank_v2::TransactionW::root(&mut b);
        t.id(1).ts(2).currency(bank_v2::Currency::EUR).from_shard(3).to_shard(4).fee(-25);
        t.entries(1).push().account(10).amount(-10);
    }
    b.finish().unwrap()
}

fn v3_msg(pool: &SimplePool) -> bytes::Bytes {
    let mut b = Builder::new(pool);
    {
        let mut t = bank_v3::TransactionW::root(&mut b);
        t.id(1).ts(2).currency(bank_v3::Currency::EUR).from_shard(3).fee(7);
        t.entries(0);
    }
    b.finish().unwrap()
}

#[test]
fn v1_reads_v2() {
    let pool = SimplePool::new(256);
    let m = v2_msg(&pool);
    let t = V1::read(&m).unwrap();
    assert_eq!((t.id(), t.from_shard(), t.to_shard()), (1, 3, 4));
    assert_eq!(t.note().unwrap(), None);
    assert_eq!(t.entries().unwrap().get(0).unwrap().amount(), -10);
}

#[test]
fn v2_reads_v1() {
    let pool = SimplePool::new(256);
    let m = v1_msg(&pool, 3);
    let t = bank_v2::Transaction::read(&m).unwrap();
    assert_eq!(t.fee(), None);
    assert_eq!(t.from_shard(), Some(3));
    assert_eq!(t.to_shard(), 4);
}

#[test]
fn v2_reads_v1_zero_is_present() {
    let pool = SimplePool::new(256);
    let m = v1_msg(&pool, 0);
    assert_eq!(bank_v2::Transaction::read(&m).unwrap().from_shard(), Some(0));
}

#[test]
fn v2_reads_v2_fee() {
    let pool = SimplePool::new(256);
    let m = v2_msg(&pool);
    assert_eq!(bank_v2::Transaction::read(&m).unwrap().fee(), Some(-25));
}

#[test]
fn v1_reads_v3_missing_required() {
    let pool = SimplePool::new(256);
    let m = v3_msg(&pool);
    assert_eq!(V1::read(&m).err(), Some(Error::Missing { field: "to_shard" }));
}

#[test]
fn v2_reads_v3() {
    let pool = SimplePool::new(256);
    let m = v3_msg(&pool);
    assert_eq!(bank_v2::Transaction::read(&m).err(), Some(Error::Missing { field: "to_shard" }));
}

#[test]
fn v3_reads_v1_and_v2() {
    let pool = SimplePool::new(256);
    let m1 = v1_msg(&pool, 9);
    let t = bank_v3::Transaction::read(&m1).unwrap();
    assert_eq!((t.from_shard(), t.fee()), (Some(9), None));
    let m = v2_msg(&pool);
    let t = bank_v3::Transaction::read(&m).unwrap();
    assert_eq!((t.from_shard(), t.fee()), (Some(3), Some(-25)));
}

#[test]
fn layouts_are_prefix_stable() {
    let v1 = [Scalar { size: 8 }, Scalar { size: 8 }, Scalar { size: 2 }, Scalar { size: 4 }, Scalar { size: 4 }, Ptr, Ptr];
    let mut v2 = v1.to_vec();
    v2.push(Scalar { size: 8 });
    let (a, b) = (compute_layout(&v1), compute_layout(&v2));
    assert_eq!(&b.fields[..v1.len()], &a.fields[..]);
    assert_eq!(b.fixed_len, bank_v2::transaction::FIXED_LEN);
    assert_eq!(b.fields[7].slot, Some(bank_v2::transaction::FEE));
    assert_eq!(b.fields[7].presence, Some((bank_v2::transaction::FEE_BIT.byte, bank_v2::transaction::FEE_BIT.shift)));
}
