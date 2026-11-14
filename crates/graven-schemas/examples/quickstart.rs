//! The example from the repository README.
//! `cargo run -p graven-schemas --example quickstart`
use graven::{Builder, SimplePool};
use graven_schemas::bank::{Currency, Transaction, TransactionW};

fn main() -> Result<(), graven::Error> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);

    // Write: one enveloped message (header + schema id + CRC32C).
    let bytes = TransactionW::to_bytes(&mut b, |t| {
        t.id(42).ts(1_700_000_000).currency(Currency::USD).from_shard(3).to_shard(7);
        t.note("rent");
        let mut entries = t.entries(2);
        entries.push().account(1001).amount(-2500).memo("transfer");
        entries.push().account(2002).amount(2500);
    })?;

    // Read: check the envelope and schema id, verify the whole message once,
    // then read fields straight from the buffer, without copying or decoding.
    let tx = Transaction::from_bytes(&bytes)?;
    println!("tx {} in {:?}, note {:?}", tx.id(), tx.currency(), tx.note()?);
    for e in tx.entries()?.iter() {
        let e = e?;
        println!("  account {} amount {} memo {:?}", e.account(), e.amount(), e.memo()?);
    }
    Ok(())
}
