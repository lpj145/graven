//! A message that writes only `id` reads every other field as its schema default, exactly,
//! at the extremes of every type: generated code and the dynamic reader agree.

use graven::{Builder, SimplePool};
use graven_codegen_tests::defaults::{Big, Extremes, ExtremesW};

fn only_id() -> Vec<u8> {
    let pool = SimplePool::new(1024);
    let mut b = Builder::new(&pool);
    ExtremesW::root(&mut b).id(1);
    b.finish().unwrap().to_vec()
}

#[test]
fn generated_reader_applies_extreme_defaults() {
    let buf = only_id();
    let e = Extremes::verified(&buf).unwrap();
    assert_eq!(e.u8_max(), u8::MAX);
    assert_eq!(e.u16_max(), u16::MAX);
    assert_eq!(e.u32_max(), u32::MAX);
    assert_eq!(e.u64_max(), u64::MAX);
    assert_eq!(e.u128_max(), u128::MAX);
    assert_eq!(e.u128_mid(), 1u128 << 127);
    assert_eq!(e.i8_min(), i8::MIN);
    assert_eq!(e.i16_min(), i16::MIN);
    assert_eq!(e.i32_min(), i32::MIN);
    assert_eq!(e.i64_min(), i64::MIN);
    assert_eq!(e.i128_min(), i128::MIN);
    assert_eq!(e.i128_max(), i128::MAX);
    assert_eq!(e.f32_max().to_bits(), f32::MAX.to_bits());
    assert_eq!(e.f32_tiny().to_bits(), 1, "smallest positive subnormal f32");
    assert_eq!(e.f32_neg0().to_bits(), (-0.0f32).to_bits());
    assert_eq!(e.f64_max().to_bits(), f64::MAX.to_bits());
    assert_eq!(e.f64_tiny().to_bits(), 1, "smallest positive subnormal f64");
    assert_eq!(e.f64_int(), -2.0);
    assert!(e.flag());
    assert_eq!(e.big(), Big::High);
}

#[test]
fn written_values_win_over_defaults() {
    let pool = SimplePool::new(1024);
    let mut b = Builder::new(&pool);
    ExtremesW::root(&mut b).id(1).u128_max(0).i128_min(0).f32_neg0(0.0).flag(false).big(Big::Low);
    let buf = b.finish().unwrap();
    let e = Extremes::verified(&buf).unwrap();
    assert_eq!((e.u128_max(), e.i128_min(), e.f32_neg0().to_bits(), e.flag(), e.big()), (0, 0, 0, false, Big::Low));
}
