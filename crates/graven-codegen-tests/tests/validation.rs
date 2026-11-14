use graven::{Builder, Invalid, Rule, SimplePool, ValidateError};
use graven_codegen_tests::validation::{Checks, Promo, PromoVerified, PromoW, Status};

struct AppChecks;

impl Checks for AppChecks {
    fn slug(&self, v: &str) -> bool {
        !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }

    fn valid_period(&self, p: &PromoVerified<'_>) -> bool {
        p.starts() < p.ends()
    }
}

#[test]
fn validate_and_to_bytes_validated_roundtrip() {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);

    // Valid message with optional fields omitted.
    let bytes = PromoW::to_bytes_validated(&mut b, &AppChecks, |p| {
        p.price(150).starts(10).ends(20);
    })
    .unwrap();

    let view = Promo::from_bytes(&bytes).unwrap();
    view.validate(&AppChecks).unwrap();
    assert_eq!(view.validate_all(&AppChecks).unwrap(), vec![]);
}

#[test]
fn validate_reports_field_index_and_table_checks() {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);

    // 1. to_bytes_validated fails on range.
    let err = PromoW::to_bytes_validated(&mut b, &AppChecks, |p| {
        p.price(0).starts(10).ends(20);
    })
    .unwrap_err();
    assert_eq!(
        err,
        ValidateError::Invalid(Invalid {
            table: "Promo",
            field: Some("price"),
            index: None,
            rule: Rule::Range,
        })
    );

    // 2. Failure inside a list of tables records `index`.
    let bytes = PromoW::to_bytes(&mut b, |p| {
        p.price(100).starts(10).ends(20);
        let mut items = p.items(2);
        items.push().sku("valid-1").qty(5).status(Status::Active);
        items.push().sku("bad sku!").qty(5).status(Status::Draft);
    })
    .unwrap();
    let view = Promo::from_bytes(&bytes).unwrap();
    assert_eq!(
        view.validate(&AppChecks).unwrap_err(),
        ValidateError::Invalid(Invalid {
            table: "Item",
            field: Some("sku"),
            index: Some(1),
            rule: Rule::Check("slug"),
        })
    );

    // 3. `known` on `[Status]` records `index`.
    let bytes = PromoW::to_bytes(&mut b, |p| {
        p.price(100).starts(10).ends(20);
        p.statuses(&[Status::Active, Status::Unknown(99)]);
    })
    .unwrap();
    let view = Promo::from_bytes(&bytes).unwrap();
    assert_eq!(
        view.validate(&AppChecks).unwrap_err(),
        ValidateError::Invalid(Invalid {
            table: "Promo",
            field: Some("statuses"),
            index: Some(1),
            rule: Rule::Known,
        })
    );

    // 4. `finite` rejects NaN on optional f32 when present, and `validate_all` collects multiple failures.
    let bytes = PromoW::to_bytes(&mut b, |p| {
        p.price(0).discount(f32::NAN).starts(20).ends(10);
    })
    .unwrap();
    let view = Promo::from_bytes(&bytes).unwrap();
    let all = view.validate_all(&AppChecks).unwrap();
    assert_eq!(
        all,
        vec![
            Invalid { table: "Promo", field: Some("price"), index: None, rule: Rule::Range },
            Invalid { table: "Promo", field: Some("discount"), index: None, rule: Rule::Range },
            Invalid { table: "Promo", field: Some("discount"), index: None, rule: Rule::Finite },
            Invalid { table: "Promo", field: None, index: None, rule: Rule::Check("valid_period") },
        ]
    );
}
