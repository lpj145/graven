use proptest::prelude::*;
use graven::{Bit, Error, TableRef, VerifyCtx};

/// Test-only recursive verifier: field at slot 0 is a nested table, field at 4 a string list.
fn walk(t: TableRef<'_>, ctx: &mut VerifyCtx) -> Result<(), Error> {
    ctx.enter()?;
    ctx.charge(t.fixed_len() as usize + 2)?;
    if let Some(c) = t.table(0)? {
        walk(c, ctx)?;
    }
    if let Some(l) = t.strs(4)? {
        for i in 0..l.len() {
            let s = l.get(i)?;
            ctx.charge(4 + s.len())?;
        }
    }
    ctx.leave();
    Ok(())
}

#[test]
fn depth_limit_hits_on_self_cycle() {
    // table at 0, fixed_len 8, slot0 (abs 2) points to table header at 0 => rel -2
    let mut b = vec![8u8, 0];
    b.extend_from_slice(&(-2i32).to_le_bytes());
    b.extend_from_slice(&0i32.to_le_bytes());
    let t = TableRef::at(&b, 0).unwrap();
    let mut ctx = VerifyCtx::new(b.len());
    assert_eq!(walk(t, &mut ctx), Err(Error::DepthExceeded));
}

#[test]
fn budget_limit_hits_on_shared_subtree() {
    // table fixed_len 8: slot0 = 0, slot4 (abs 6) -> list at 10: [50][50 × ptr to same string]
    let mut b = vec![8u8, 0, 0, 0, 0, 0];
    b.extend_from_slice(&4i32.to_le_bytes()); // abs 6 -> 10
    b.extend_from_slice(&50u32.to_le_bytes()); // 10..14
    let str_pos = 14 + 50 * 4;
    for i in 0..50usize {
        let slot = 14 + i * 4;
        b.extend_from_slice(&((str_pos - slot) as i32).to_le_bytes());
    }
    b.extend_from_slice(&10u32.to_le_bytes());
    b.extend_from_slice(b"0123456789");
    let t = TableRef::at(&b, 0).unwrap();
    let mut ctx = VerifyCtx::with_limits(64, 100);
    assert_eq!(walk(t, &mut ctx), Err(Error::BudgetExceeded));
    let mut ok = VerifyCtx::new(b.len());
    assert_eq!(walk(t, &mut ok), Ok(()));
}

#[test]
fn default_limits() {
    let c = VerifyCtx::new(10);
    assert_eq!(c.max_depth(), 64);
    assert_eq!(c.budget(), 1 << 20);
    assert_eq!(VerifyCtx::new(1 << 20).budget(), 8 << 20);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if cfg!(miri) { 64 } else { 4096 }))]
    #[test]
    fn arbitrary_bytes_never_panic(b in proptest::collection::vec(any::<u8>(), 0..512), byte in 0u16..40, shift in 0u8..7) {
        for pos in 0..b.len().min(8) {
            if let Ok(t) = TableRef::at(&b, pos) {
                let bit = Bit { byte, shift };
                for off in 0u16..32 {
                    let _ = t.scalar::<u64>(off, bit);
                    let _ = t.scalar::<u8>(off, bit);
                    let _ = t.scalar_or::<i32>(off, bit, 0);
                    let _ = t.bool(bit);
                    let _ = t.str(off);
                    let _ = t.bytes(off);
                    let _ = t.union(off);
                    if let Ok(Some(c)) = t.table(off) { let _ = c.scalar::<u16>(0, bit); }
                    if let Ok(Some(l)) = t.scalars::<u32>(off) { let _ = l.iter().count(); let _ = l.get(l.len()); }
                    if let Ok(Some(l)) = t.tables(off) {
                        for e in l.iter().take(64).flatten() { let _ = e.str(0); }
                    }
                    if let Ok(Some(l)) = t.strs(off) { for i in 0..l.len().min(64) { let _ = l.get(i); } }
                    let mut ctx = VerifyCtx::new(b.len());
                    let _ = walk(t, &mut ctx);
                }
            }
        }
    }
}
