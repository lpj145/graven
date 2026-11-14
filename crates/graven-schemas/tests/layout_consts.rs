use graven::Bit;
use graven_layout::{FieldKind::*, FieldLayout, compute_layout};
use graven_schemas::{bank, game, iot, stream};

fn s(off: u16, bit: Bit) -> FieldLayout {
    FieldLayout { slot: Some(off), presence: Some((bit.byte, bit.shift)) }
}
fn p(off: u16) -> FieldLayout {
    FieldLayout { slot: Some(off), presence: None }
}
fn b(bit: Bit) -> FieldLayout {
    FieldLayout { slot: None, presence: Some((bit.byte, bit.shift)) }
}

#[test]
fn bank_entry() {
    use bank::entry::*;
    let l = compute_layout(&[Scalar { size: 8 }, Scalar { size: 8 }, Ptr]);
    assert_eq!(l.fixed_len, FIXED_LEN);
    assert_eq!(l.fields, [s(ACCOUNT, ACCOUNT_BIT), s(AMOUNT, AMOUNT_BIT), p(MEMO)]);
    assert_eq!(PTR_SLOTS, &[MEMO]);
}

#[test]
fn bank_transaction() {
    use bank::transaction::*;
    let l = compute_layout(&[
        Scalar { size: 8 }, Scalar { size: 8 }, Scalar { size: 2 }, Scalar { size: 4 }, Scalar { size: 4 }, Ptr, Ptr,
    ]);
    assert_eq!(l.fixed_len, FIXED_LEN);
    assert_eq!(
        l.fields,
        [s(ID, ID_BIT), s(TS, TS_BIT), s(CURRENCY, CURRENCY_BIT), s(FROM_SHARD, FROM_SHARD_BIT), s(TO_SHARD, TO_SHARD_BIT), p(ENTRIES), p(NOTE)]
    );
}

#[test]
fn game_entity_and_frame() {
    {
        use game::entity::*;
        let l = compute_layout(&[Scalar { size: 4 }, Scalar { size: 12 }, Scalar { size: 12 }, Scalar { size: 2 }, Bool]);
        assert_eq!(l.fixed_len, FIXED_LEN);
        assert_eq!(l.fields, [s(ID, ID_BIT), s(POS, POS_BIT), s(VEL, VEL_BIT), s(HP, HP_BIT), b(ALIVE_BIT)]);
        assert!(PTR_SLOTS.is_empty());
    }
    use game::frame::*;
    let l = compute_layout(&[Scalar { size: 8 }, Ptr]);
    assert_eq!(l.fixed_len, FIXED_LEN);
    assert_eq!(l.fields, [s(TICK, TICK_BIT), p(ENTITIES)]);
}

#[test]
fn iot_telemetry() {
    use iot::telemetry::*;
    let l = compute_layout(&[Scalar { size: 4 }, Scalar { size: 8 }, Scalar { size: 4 }, Scalar { size: 4 }, Scalar { size: 1 }]);
    assert_eq!(FIXED_LEN, 22);
    assert_eq!(l.fixed_len, FIXED_LEN);
    assert_eq!(
        l.fields,
        [s(DEVICE, DEVICE_BIT), s(TS, TS_BIT), s(TEMP, TEMP_BIT), s(HUMIDITY, HUMIDITY_BIT), s(BATTERY, BATTERY_BIT)]
    );
}

#[test]
fn stream_chunk() {
    use stream::chunk::*;
    let l = compute_layout(&[Scalar { size: 8 }, Scalar { size: 4 }, Ptr]);
    assert_eq!(l.fixed_len, FIXED_LEN);
    assert_eq!(l.fields, [s(STREAM_ID, STREAM_ID_BIT), s(SEQ, SEQ_BIT), p(DATA)]);
}
