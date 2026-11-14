# Coming from JSON

If your data is JSON today, you already know its shape. This page translates that shape
into a Graven schema, piece by piece, using a small product catalog. Every command and
output below is real.

## The data

A catalog as most APIs would send it: an object with a list of products.

```json
{
  "products": [
    { "sku": "CF-01", "name": "Coffee beans 1kg", "price": { "cents": 8990, "currency": 986 },
      "tags": ["coffee", "beans"], "stock": 120 },
    { "sku": "MG-02", "name": "Ceramic mug", "price": { "cents": 3500, "currency": 986 },
      "status": "Draft" }
  ],
  "updated_at": 1700000000
}
```

## The schema

```graven
namespace shop;

enum Status : u8 { Draft = 0, Active = 1, Archived = 2 }

struct Money { cents: i64, currency: u16 }

table Product {
  sku    @0: string;
  name   @1: string;
  price  @2: Money;
  status @3: Status = Active;
  tags   @4: [string]?;
  stock  @5: u32?;
}

table Catalog {
  products   @0: [Product];
  updated_at @1: u64?;
}
```

Each decision, against the JSON it came from:

- **`Product` and `Catalog` are tables.** A JSON object whose keys you know becomes a
  table. Every field gets an ID (`@0`, `@1`, …); new fields are added later with new IDs at
  the end, and old readers keep working.
- **`price` is a struct.** `cents` and `currency` always travel together and will never
  change, so they are stored inline, side by side, with no per-field overhead. Use a
  struct only for groups like this; anything that may grow is a table.
- **`price.cents` is an integer.** JSON has one number type; here you choose. Money is an
  integer number of cents, never a float.
- **`status` is an enum with a default.** A fixed set of strings becomes an enum: one byte
  instead of a string. The first product has no `status` in the JSON, so it reads as the
  default, `Active`.
- **`tags` and `stock` are optional (`?`).** In JSON these keys are sometimes missing. In
  Graven, absent is recorded on the wire, so a stock of `0` is different from no stock.
- **`sku` and `name` are required.** A reader rejects a message without them.

## JSON concepts and their Graven counterparts

| In JSON | In Graven | Notes |
|---|---|---|
| object with known keys | `table` | fields have IDs; add new ones at the end |
| small group of values that never changes | `struct` | stored inline, frozen forever |
| array | `[T]` | lists of scalars, enums, structs, strings, bytes or tables |
| string | `string` | UTF-8, validated by `verify` |
| base64 or binary data | `bytes` | raw bytes, no encoding overhead |
| number | `u8` … `u128`, `i8` … `i128`, `f32`, `f64` | pick the range you need; money as integer cents |
| `true` / `false` | `bool` | |
| a fixed set of strings | `enum` | stored as an integer; unknown values survive |
| a key that may be missing, or `null` | optional field `T?` | absent is different from zero or empty |
| a missing key that means a known value | default `T = x` | the default is never written |
| `"type": "..."` choosing between shapes | `union` of tables | an unknown variant is reported, not an error |
| an object used as a map (`{"BRL": 5, "USD": 1}`) | a list of entry tables | `table Rate { code @0: string; value @1: i64; }` and `rates @0: [Rate]` |
| `null` inside an array | not available | lists have no absent elements; use a table with an optional field |

## Converting existing JSON

`gravenc` converts JSON into Graven messages and back, using the schema:

```sh
gravenc update catalog.graven --init        # create the lockfile once
gravenc from-json catalog.graven --root Catalog catalog.json -o catalog.bin
```

```text
catalog.bin: 1 message(s), 185 bytes
```

The minified JSON is 248 bytes; the Graven message is 185, including its 24-byte envelope
with schema id and checksum. To look inside:

```text
$ gravenc inspect catalog.graven catalog.bin
#0  offset 0  shop.Catalog  185 bytes
{
  "products": [
    {
      "sku": "CF-01",
      "name": "Coffee beans 1kg",
      "price": {
        "cents": 8990,
        "currency": 986
      },
      "status": "Active",
      "tags": [
        "coffee",
        "beans"
      ],
      "stock": 120
    },
    {
      "sku": "MG-02",
      "name": "Ceramic mug",
      "price": {
        "cents": 3500,
        "currency": 986
      },
      "status": "Draft"
    }
  ],
  "updated_at": 1700000000
}
```

The first product shows `"status": "Active"`: it was absent, so the reader applies the
default.

## A top-level array

Plenty of JSON is just an array: `[{"sku": "CF-01", …}, {"sku": "MG-02", …}]`. A Graven
message always starts at a table, so there are two ways to store it.

**Wrap it in a table**, as `Catalog` does above. The whole list is one message: choose this
when the list is read and written as a unit, like an API response or a configuration.
[Binary layout](binary-layout.md#lists-of-tables) shows such a list byte by byte.

**Store one message per element.** `from-json` reads a top-level array (or JSON Lines) as
one message per element and writes them one after another, each in its own envelope:

```sh
gravenc from-json catalog.graven --root Product products.json -o products.bin
```

```text
products.bin: 2 message(s), 192 bytes
```

Choose this for collections that grow: logs, events, exports. New records are appended
without rewriting the file, each record is checked on its own, and
[`gravenc scan`](tools.md#scan) tells a crash-truncated tail from real corruption.

```text
$ gravenc to-json catalog.graven products.bin
{"sku":"CF-01","name":"Coffee beans 1kg","price":{"cents":8990,"currency":986},"status":"Active","tags":["coffee","beans"],"stock":120}
{"sku":"MG-02","name":"Ceramic mug","price":{"cents":3500,"currency":986},"status":"Draft"}
```

## The same data from Rust

With the code `gravenc` generates for `catalog.graven` (see
[getting started](getting-started.md)):

```rust
use graven::{Builder, SimplePool};
use shop::{Catalog, CatalogW, Money, Status};

fn main() -> Result<(), graven::Error> {
    let pool = SimplePool::new(4096);
    let mut b = Builder::new_enveloped(&pool);
    let bytes = CatalogW::to_bytes(&mut b, |c| {
        c.updated_at(1_700_000_000);
        let mut products = c.products(2);
        products
            .push()
            .sku("CF-01")
            .name("Coffee beans 1kg")
            .price(Money { cents: 8990, currency: 986 })
            .tags(["coffee", "beans"])
            .stock(120);
        products
            .push()
            .sku("MG-02")
            .name("Ceramic mug")
            .price(Money { cents: 3500, currency: 986 })
            .status(Status::Draft);
    })?;

    let catalog = Catalog::from_bytes(&bytes)?;
    for p in catalog.products()?.iter() {
        let p = p?;
        println!("{} {} {:?} stock={:?}", p.sku()?, p.name()?, p.status(), p.stock());
    }
    Ok(())
}
```

```text
CF-01 Coffee beans 1kg Active stock=Some(120)
MG-02 Ceramic mug Draft stock=None
```

These bytes are identical to the `catalog.bin` that `gravenc from-json` produced from the
JSON: the tools and the generated code follow the same canonical layout.

## What works differently from JSON

- **Names are not in the bytes; IDs are.** Renaming a field is free. Reusing an ID, or
  changing a type, is refused by `gravenc update`.
- **Types are fixed by the schema.** A field cannot be a number in one message and a
  string in the next. Evolution follows the
  [compatibility rules](spec/graven-format-v1.md#9-schema-compatibility).
- **Absent is explicit.** Every field knows whether it was written, so `0`, `""` and
  "not sent" are three different things.
- **Reading is free until you touch a field.** There is no parse step: fields are read in
  place, and one `verify` makes untrusted input safe to read.
