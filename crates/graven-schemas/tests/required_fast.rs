//! The mask fast path in generated `from_ref` must agree with the slow per-field check.
use proptest::prelude::*;
use graven::TableRef;
use graven_schemas::{FromTable, Req, check_required};

fn agree<'a, T: FromTable<'a>>(buf: &'a [u8], reqs: &[Req]) -> Result<(), TestCaseError> {
    if let Ok(t) = TableRef::at(buf, 0) {
        let fast = T::from_ref(t).map(|_| ());
        let slow = check_required(&t, reqs);
        prop_assert_eq!(fast, slow);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if cfg!(miri) { 16 } else { 8192 }))]
    #[test]
    fn fast_and_slow_required_checks_agree(len in 0u16..60, body in proptest::collection::vec(any::<u8>(), 60), sparse in any::<bool>()) {
        let mut buf = len.to_le_bytes().to_vec();
        // sparse: mostly-zero bodies so pointer slots are often 0 (absent)
        buf.extend(body.iter().map(|b| if sparse && b % 4 != 0 { 0 } else { *b }));
        use graven_schemas::{bank, evo, game, iot, stream};
        agree::<bank::Transaction>(&buf, bank::TRANSACTION_REQ)?;
        agree::<bank::Entry>(&buf, bank::ENTRY_REQ)?;
        agree::<game::Frame>(&buf, game::FRAME_REQ)?;
        agree::<game::Entity>(&buf, game::ENTITY_REQ)?;
        agree::<iot::Telemetry>(&buf, iot::TELEMETRY_REQ)?;
        agree::<stream::Chunk>(&buf, stream::CHUNK_REQ)?;
        agree::<evo::bank_v2::Transaction>(&buf, evo::bank_v2::TRANSACTION_REQ)?;
        agree::<evo::bank_v3::Transaction>(&buf, evo::bank_v3::TRANSACTION_REQ)?;
    }
}
