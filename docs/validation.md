# Validation

Graven is a language-neutral binary format, and its validation model is designed so
every language agrees on the same rules without changing the wire format:

- **Structure (`verify`):** buffer bounds, UTF-8, required fields, depth and budget. It runs
  when bytes are opened, because reading malformed bytes would be unsafe.
- **Value rules (`validate`):** domain rules declared in the schema (`range`, `len`, `finite`,
  `known`, `check(name)`). They **never run automatically on read**, so data written under
  looser rules stays readable; a program runs them when it asks.

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

### In each language

The schema stays identical; each SDK turns `check(name)` into its native callback
interface, backed by whatever library your project already uses:

<!-- tabs -->
#### Rust

```rust
use shop::{Checks, Promo, PromoVerified, PromoW};

struct AppChecks;

// Ok(()) accepts the value; Err(code) rejects it with a reason code.
impl Checks for AppChecks {
    fn email(&self, v: &str) -> Result<(), &'static str> {
        if v.contains('@') { Ok(()) } else { Err("missing_at") }
    }
    fn slug(&self, v: &str) -> Result<(), &'static str> {
        if v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { Ok(()) } else { Err("bad_char") }
    }
    fn valid_period(&self, p: &PromoVerified<'_>) -> Result<(), &'static str> {
        if p.starts() < p.ends() { Ok(()) } else { Err("ends_before_start") }
    }
}

let bytes = PromoW::to_bytes_validated(&mut builder, &AppChecks, |p| {
    p.name("summer-sale").contact("hi@example.com").starts(10).ends(20);
})?;

let view = Promo::from_bytes(&bytes)?;
view.validate(&AppChecks)?;               // stops at first failure
let all = view.validate_all(&AppChecks);  // collects every failure
```

*(Details: [Rust SDK validation](sdk/rust.md#validation).)*

#### JavaScript / TypeScript

```ts
import { z } from "zod";
import type { Checks } from "./gen/shop.js";
import { Promo, PromoW } from "./gen/shop.js";

// `true` accepts the value; a string rejects it with that reason code.
const checks: Checks = {
  email: (v) => z.string().email().safeParse(v).success || "not_an_email",
  slug: (v) => /^[a-z0-9-]+$/.test(v) || "bad_char",
  valid_period: (p) => p.starts() < p.ends() || "ends_before_start",
};

const bytes = PromoW.toBytesValidated(
  (p) => p.name("summer-sale").contact("hi@example.com").starts(10n).ends(20n),
  checks,
);

const promo = Promo.fromBytes(bytes);
promo.validate(checks);                // throws ValidationError at first failure
const all = promo.validateAll(checks); // returns ValidationFailure[]
```

*(Details: [JS / TS SDK validation](sdk/js.md#validation).)*
<!-- /tabs -->

## 3. Running validation

Validation runs only when asked, in every implementation:

- **when writing**, a validated write builds the message, verifies it and runs the rules
  before returning the bytes;
- **when reading**, opening bytes checks structure only; the program calls `validate` (stop
  at the first failure) or `validate_all` (collect every failure) when it needs the rules.

Every failure reports:

| | |
|---|---|
| table | where the rule is declared |
| field | the field, or none for a table-level check |
| index | the list position, when the value sits in a list (`[Enum]` or `[Table]`) |
| rule | `range`, `len`, `finite`, `known` or `check(name)` |
| min, max | for `range` and `len`: the bounds declared in the schema, as decimal numbers |
| reason | for `check(name)`: the code the check returned |

Services carry the same report in the `Invalid` fault
([Services](services.md#errors-and-faults)).

### Messages in the reader's language

A failure carries codes, never text. The schema is the same in every language, and a server
does not know who reads its reply, so neither of them writes messages. The program that shows
the failure turns the codes into a message in its user's language, with the translation
library it already uses:

```ts
// en.json: "rule.len": "Must be {{min}} to {{max}} characters long",
//          "reason.missing_at": "Missing @"
// pt.json: "rule.len": "Deve ter entre {{min}} e {{max}} caracteres",
//          "reason.missing_at": "Falta o @"
function message(f: ValidationFailure): string {
  if (f.reason !== undefined) return i18n.t(`reason.${f.reason}`);
  return i18n.t(`rule.${f.rule}`, { min: f.min, max: f.max });
}
```

The same function serves a failure from `validate` and the `broken` part of an `Invalid`
fault from a server.

A schema without rules generates no validation code.

## 4. Evolution

The lockfile (`<schema>.graven.lock`) records every field's rules and each table's `check`s,
and `gravenc update` checks how they change:

- **Allowed:** rules on a new field or table; loosening a `range`/`len` bound; removing a rule or `check(name)`.
- **Warning (`rule-tightened` / `check-added`):** tightening bounds, adding a rule to an
  existing field, or adding `check(name)` to an existing field or table. Confirm with
  `#[allow_breaking_readers]` on that field or table.

## 5. `gravenc inspect`

`gravenc inspect` checks the built-in rules (`range`, `len`, `finite`, `known`) of the
messages it reads and reports each `check(name)` as `not checked`: named checks live in the
application, not in the schema.
