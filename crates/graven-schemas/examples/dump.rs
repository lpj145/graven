//! Build one `Transaction`, print its bytes annotated, and read it back.
//! `cargo run -p graven-schemas --example dump`
use graven::{Builder, SimplePool};
use graven_schemas::bank::{Currency, Transaction, TransactionW};

fn main() {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new(&pool);
    {
        let mut t = TransactionW::root(&mut b);
        t.id(42).ts(1_700_000_000).currency(Currency::BRL).from_shard(3).to_shard(7);
        let mut l = t.entries_streaming(); // count unknown up front
        l.push().account(1001).amount(-2500).memo("transfer");
        l.push().account(2002).amount(2500);
        l.close();
        t.note("rent");
    }
    let msg = b.finished().unwrap();

    println!("{} bytes", msg.len());
    for (i, row) in msg.chunks(16).enumerate() {
        let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
        println!("{:04x}: {}", i * 16, hex.join(" "));
    }

    let t = Transaction::read(msg).unwrap();
    println!(
        "\nid={} ts={} currency={:?} from={} to={} note={:?}",
        t.id(), t.ts(), t.currency(), t.from_shard(), t.to_shard(), t.note().unwrap()
    );
    for e in t.entries().unwrap().iter() {
        let e = e.unwrap();
        println!("  entry account={} amount={} memo={:?}", e.account(), e.amount(), e.memo().unwrap());
    }
}
