//! `orders.graven` imports `common`: its generated code uses `common`'s own Rust types for
//! structs, enums, tables and unions, and messages round-trip through them.

use graven::{Builder, SimplePool};
use graven_codegen_tests::common::{self, Contact, ContactVerified, Currency, Money, Uuid};
use graven_codegen_tests::orders::{DeliveryVerified, Order, OrderW};

#[test]
fn imported_types_are_the_owners_types() {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);
    let bytes = OrderW::to_bytes(&mut b, |o| {
        o.id(Uuid { hi: 1, lo: 2 }).total(Money { cents: 1990, currency: Currency::BRL });
        o.ship_to().street("Rua A").city("Recife");
        let mut lines = o.lines(1);
        lines.push().sku("CF-01").unit(Money { cents: 1990, currency: Currency::BRL });
        drop(lines);
        o.contact_email().address("a@b.c");
        let mut h = o.history(1);
        h.push().street("Rua B").city("Olinda").country(Currency::USD);
        drop(h);
        o.delivery_address().street("Rua C").city("Natal");
    })
    .unwrap();

    let o = Order::from_bytes(&bytes).unwrap();
    // These annotations are the point: the accessors return `common`'s types.
    let id: Uuid = o.id();
    let total: Money = o.total();
    assert_eq!((id.hi, id.lo, total.cents, total.currency), (1, 2, 1990, Currency::BRL));
    assert_eq!(o.currency(), Currency::USD, "imported enum default");

    let ship: common::AddressVerified<'_> = o.ship_to().unwrap().unwrap();
    assert_eq!((ship.street().unwrap(), ship.city().unwrap(), ship.country()), ("Rua A", "Recife", Currency::BRL));

    let line = o.lines().unwrap().get(0).unwrap();
    assert_eq!((line.sku().unwrap(), line.qty(), line.unit().cents), ("CF-01", 1, 1990));

    match o.contact().unwrap() {
        Some(ContactVerified::Email(e)) => assert_eq!(e.address().unwrap(), "a@b.c"),
        _ => panic!("expected the Email variant"),
    }
    let hist = o.history().unwrap().unwrap();
    assert_eq!(hist.get(0).unwrap().country(), Currency::USD);
    match o.delivery().unwrap() {
        Some(DeliveryVerified::Address(a)) => assert_eq!(a.city().unwrap(), "Natal"),
        _ => panic!("expected the Address variant"),
    }

    // The checked reader returns the owner's union type too.
    let checked = o.reader();
    assert!(matches!(checked.contact().unwrap(), Some(Contact::Email(_))));
}

/// An imported table keeps the schema id of the namespace that declares it.
#[test]
fn imported_tables_keep_their_schema_id() {
    assert_eq!(common::Address::SCHEMA_ID, graven::envelope::schema_id("common.Address"));
    assert_eq!(Order::SCHEMA_ID, graven::envelope::schema_id("orders.Order"));
}
