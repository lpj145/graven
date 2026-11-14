# Validation

Graven is a language-neutral binary format, and its validation model is designed so
every language agrees on the same rules without changing the wire format:

- **Structural (`verify` / `from_bytes`):** checks buffer bounds, UTF-8, required fields, depth
  and budget. Runs on read to guarantee memory safety.
- **Value rules (`validate` / `validate_all` / `to_bytes_validated`):** checks domain rules
  declared in the schema (`range`, `len`, `finite`, `known`, `check(name)`). **Never runs
  automatically on read**, so old data written under looser rules stays readable.

## 1. Built-in rules (portable across languages)

Declare built-in rules directly on table fields (structs are frozen and cannot carry rules):

```graven
namespace shop;

enum Status : u8 { Active = 0, Draft = 1 }

table Product {
  #[len(min = 1, max = 64)]
  name @0: string;

  #[range(min = 1, max = 10_000_000)]
  price @1: u32;

  #[finite, range(min = 0.0, max = 100.0)]
  discount @2: f32?;

  #[known]
  status @3: Status;
}
```

| Rule | Applies to | Meaning |
|---|---|---|
| `range(min = a, max = b)` | integers (`u8`..`u128`, `i8`..`i128`) and floats (`f32`, `f64`) | `a ≤ value ≤ b` (inclusive; `NaN` fails). Either bound may be omitted. |
| `len(min = a, max = b)` | `string`, `bytes`, `[T]` | **Byte** length (`string`/`bytes`) or element count (`[T]`). Either bound may be omitted. |
| `finite` | `f32`, `f64` | Rejects `NaN` and infinities. |
| `known` | `enum` and `[Enum]` | Rejects unknown enum variants (`Unknown(_)`). |

- `len` counts **UTF-8 bytes** rather than characters, because languages disagree on code
  points vs. grapheme clusters while byte counts are exact everywhere.
- Rules on an optional field (`?`) run **only when the field is present**.
- A field's default value must satisfy its own rules.
- If a schema uses **only** built-in rules (no `check(...)`), no application hooks are needed.

## 2. Custom checks (`check(name)`)

Graven intentionally has **no expression language** (like CEL) and **no regexes** in schemas:
regex dialects and evaluators differ across languages. Instead, `#[check(name)]` declares a
named hook in the schema, and each language implements it with its own standard libraries:

```graven
namespace shop;

#[check(valid_period)]
table Promo {
  #[len(min = 1, max = 64), check(slug)]
  name @0: string;

  #[check(email)]
  contact @1: string;

  starts @2: u64;
  ends   @3: u64;
}
```

- On a **field**, `check(name)` receives the field's value (e.g. string, integer, list).
- On a **table**, `check(name)` receives the verified table view.
- A check name must always have the same input type across the schema.

### In Rust (reference implementation)

From `Promo`, `gravenc` generates a `Checks` trait in `mod shop`:

```rust
// Generated in `mod shop` (do not write this yourself):
pub trait Checks {
    fn email(&self, v: &str) -> bool;
    fn slug(&self, v: &str) -> bool;
    fn valid_period(&self, v: &PromoVerified<'_>) -> bool;
}
```

Implement `Checks` in your application (delegating to crates like `regex` or `validator`):

```rust
use shop::{Checks, Promo, PromoVerified, PromoW};

struct AppChecks;

impl Checks for AppChecks {
    fn email(&self, v: &str) -> bool {
        v.contains('@') // or delegate to `validator::ValidateEmail`
    }

    fn slug(&self, v: &str) -> bool {
        !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }

    fn valid_period(&self, p: &PromoVerified<'_>) -> bool {
        p.starts() < p.ends()
    }
}
```

*(If a schema has only built-in rules, `Checks` is implemented for `()` so you can pass `&()`.)*

### In other languages

The schema stays identical. Any language generator maps `#[check(name)]` to its own
interface or callback object — for example, in TypeScript (once a TypeScript generator
exists), delegating to Zod:

```ts
const checks: shop.Checks = {
  email: (v) => z.string().email().safeParse(v).success,
  slug: (v) => /^[a-z0-9-]+$/.test(v),
  valid_period: (p) => p.starts() < p.ends(),
};
```

## 3. Running validation

Validation runs only when asked:

```rust
// On write: builds, verifies structure, and runs `AppChecks` + built-in rules.
let bytes = PromoW::to_bytes_validated(&mut builder, &AppChecks, |p| {
    p.name("summer-sale").contact("hi@example.com").starts(10).ends(20);
})?;

// On read: `from_bytes` checks structure only; call `validate` when needed.
let view = Promo::from_bytes(&bytes)?;
view.validate(&AppChecks)?;                  // stops at first failure (no allocation)
let all = view.validate_all(&AppChecks)?;    // collects every failure
```

Every failure reports the table, optional field, optional list index (for `[Enum]` or `[Table]`),
and the rule that failed (`Range`, `Len`, `Finite`, `Known`, or `Check("slug")`).

Schemas without any rules generate the exact same code as before.

## 4. Evolution (`graven.lock`)

`graven.lock` records every field's rules and each table's `check`s:

- **Allowed:** rules on a new field or table; loosening a `range`/`len` bound; removing a rule or `check(name)`.
- **Warning (`rule-tightened` / `check-added`):** tightening bounds, adding a rule to an
  existing field, or adding `check(name)` to an existing field or table. Confirm with
  `#[allow_breaking_readers]` on that field or table.

## 5. Dynamic validation & `gravenc inspect`

- **`graven-dynamic`:** `Validator::new(root, schema, registry)` checks at construction time
  that every reachable `check(name)` is registered in `CheckRegistry`.
- **`gravenc inspect`:** checks built-in rules (`range`, `len`, `finite`, `known`) and
  reports each `check(name)` as `not checked`.
