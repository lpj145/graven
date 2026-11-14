# Proposal 0002: validation rules in schemas

Status: **accepted and implemented** in 0.2.0 (see [Validation](../validation.md)).

## Motivation

`verify` checks that bytes form a well-formed message: pointers in bounds, valid UTF-8,
required fields present, depth and work limits. It says nothing about whether the values
make sense. A price of zero, a 10 MB product name or a list of a million tags pass
`verify`, and every application has to repeat these checks in its own code, in every
language it uses.

Rules like "a name has 1 to 64 bytes" belong next to the field they describe. Schemas can
carry them without touching the wire format.

## Goals

- Declare common value rules in the schema: numeric ranges and lengths.
- Allow any other rule, including ones that need a regular expression or several fields,
  through **named functions** that each application provides.
- Make the compiler check rule changes over time, as it already does for layouts. A
  tighter rule can turn data written last year into invalid data.
- Keep the format unchanged: no change to format v1, the specification's wire rules or the
  conformance vectors.

## Non-goals

- **No expression language** in schemas (such as CEL in `protovalidate`). Every
  implementation would need its own evaluator, and they would have to agree on every edge
  case.
- **No regular expressions** in schemas. Regex dialects differ between languages, so a
  pattern can match in Rust and fail in JavaScript. Named functions cover this case
  instead.
- **No automatic rejection on read.** See [Reading old data](#reading-old-data).

## Syntax

Rules are attributes. The existing attribute syntax is extended with arguments and with
several attributes in one bracket:

```graven
table Product {
  #[len(min = 1, max = 64), check(slug)]
  name  @0: string;

  #[range(min = 1, max = 10_000_000)]
  price @1: u32;

  #[len(max = 100)]
  tags  @2: [string]?;
}

#[check(valid_period)]
table Promo {
  starts @0: u64;
  ends   @1: u64;
}
```

Grammar changes (to section 1 of the language guide):

```text
attrs     = "#" "[" attr { "," attr } "]" ;
attr      = ident [ "(" [ arg { "," arg } ] ")" ] ;
arg       = ident [ "=" literal ] ;
table     = { attrs } "table" ident "{" { tfield } "}" ;
```

`#[allow_breaking_readers]` keeps its meaning and can share a bracket with rules.

## Built-in rules

| Rule | Applies to | Meaning |
|---|---|---|
| `range(min = a, max = b)` | integer and float fields | `a ≤ value ≤ b`; either bound may be left out |
| `len(min = a, max = b)` | `string`, `bytes` | length in **bytes** (UTF-8 bytes for strings) |
| `len(min = a, max = b)` | lists | number of elements |
| `finite` | `f32`, `f64` | not NaN and not infinite |
| `known` | enum fields and lists of enums | the value is a variant the reader's schema declares |

`known` exists for applications that must not accept variants added after they were
built. Without it, an unknown value reads as `Unknown(n)` and passes `validate`. Adding a
variant to an enum is still allowed, but a reader whose schema predates it and that uses
`known` will report data with the new variant as invalid. That is the point of the rule.

Bounds are inclusive. Lengths count bytes rather than characters because "character" has
several meanings (code points, grapheme clusters) that languages disagree on, while a byte
count is exact everywhere.

The compiler checks rules against their field: `range` on a string is an error, a bound
must fit the field's type, `min` must not exceed `max`, and a field's default must satisfy
its own rules.

## Named functions

`check(name)` refers to a function the application provides. The schema fixes only its
name and what it receives; the body lives in the application, in each language it uses.

| Placed on | The function receives |
|---|---|
| a scalar, `string` or `bytes` field | the value (`u32`, `&str`, `&[u8]`, …) |
| a list field | the whole list; rules on each element (each tag at most 30 bytes) are written here |
| a table | the verified table, for rules across fields |

A name always has the same input type. Using `slug` on a `string` in one place and on a
`u32` in another is an error. A function returns pass or fail. The message that explains a
failure comes from the application.

### In Rust

The generated code declares one trait per schema file, with one method per function name:

```rust
// generated
pub trait Checks {
    fn slug(&self, v: &str) -> bool;
    fn valid_period(&self, p: &PromoVerified<'_>) -> bool;
}
```

The application implements it once:

```rust
struct ShopChecks { slug: regex::Regex }

impl shop::Checks for ShopChecks {
    fn slug(&self, v: &str) -> bool { self.slug.is_match(v) }
    fn valid_period(&self, p: &shop::PromoVerified<'_>) -> bool { p.starts() < p.ends() }
}
```

A function that is missing is a compile error, not a failure discovered in production.

### Using existing validation libraries

Named functions do not replace the validation libraries applications already use; they
call them. Each function is usually one line that delegates:

```rust
// Rust, with the `validator` crate
use validator::ValidateEmail;

impl shop::Checks for ShopChecks {
    fn email(&self, v: &str) -> bool { v.validate_email() }
    // …
}
```

```ts
// TypeScript, with Zod (once a TypeScript implementation exists)
const checks: shop.Checks = {
  email: (v) => z.string().email().safeParse(v).success,
};
```

The schema stays the same in every language: it names `email`, and each language uses its
usual library. A validation package for Graven is not needed.

### In the dynamic tools

`graven-dynamic` and other implementations without generated code take a registry of
functions by name. Building a validator with a name missing from the registry is an error
**at construction**, never halfway through validating a message.

`gravenc` cannot run application code. Its commands check the built-in rules and report
each named function as "not checked", rather than as passed.

## When rules run

### Writing

Validation needs the application's functions, so it is a separate step after building.
The generated API adds:

```rust
let bytes = ProductW::to_bytes_validated(&mut b, &checks, |p| {
    p.name("pen").price(150);
})?;   // Err(Invalid { … }) and nothing is returned if a rule fails
```

`to_bytes` keeps its current signature and does not validate.

### Reading

The verified view gets `validate`:

```rust
let p = Product::from_bytes(&bytes)?;   // structure, as today
p.validate(&checks)?;                   // rules, only when asked
```

### Reading old data

Reading never fails because of a rule. Data written years ago was valid under the rules of
its time. If reading applied today's rules automatically, a stricter rule would make old
files unreadable. `verify` stays structural. Each application decides when to call
`validate` and what to do with an old record that fails it.

### Optional fields

A rule on an optional field applies only when the field is present. To require a value,
make the field required.

### The error

```rust
pub struct Invalid {
    pub table: &'static str,
    pub field: Option<&'static str>,   // None for a table-level check
    pub index: Option<u32>,            // the element, when the failing value is in a list
    pub rule: Rule,                    // Range, Len, Finite or Check("slug")
}
```

`validate` stops at the first failure and needs no allocation. It returns
`graven::ValidateError`, which is either `Invalid` (a broken rule) or `Read` (the message
could not be read); `to_bytes_validated` returns the same type. A `validate_all` variant
collects every failure, for reporting.

## Evolution and the lockfile

The lockfile records every field's and table's rules. `gravenc update` compares them with
the previous lock, like every other change:

| Change | Verdict |
|---|---|
| a new field or table with rules | allowed |
| a rule loosened (a wider range, a longer maximum) or removed | allowed |
| `check(f)` removed | allowed |
| a rule tightened, a rule added to an existing field, `check(f)` or `known` added | warning: confirm with `#[allow_breaking_readers]` |
| a function renamed | a removal and an addition, so a warning |

New rule names: `rule-tightened` and `check-added` for the warnings. Tightening is a
warning rather than an error because it is often intended. Its cost, that data written
before may now fail `validate`, has to be accepted explicitly.

The lockfile cannot see inside a function. Changing what `slug` accepts is invisible to
the compiler. The language guide will say so: a change to a function's behaviour is a
rule change and deserves a new name.

## What does not change

- The wire format, the specification's sections on encoding, and the conformance vectors.
- `verify`, `read`, `from_bytes` and `to_bytes`.
- Schemas without rules: they compile to the same code as today.

## Alternatives considered

- **Regular expressions in the schema.** Rejected: dialects differ between languages, and
  a format meant for several languages cannot depend on one. A named function backed by a
  regex gives the same result, with the dialect chosen by each application.
- **An expression language** (CEL, as in `protovalidate`). Rejected for now: it needs an
  evaluator in every implementation, and the evaluators must agree exactly. Named
  functions cover the same cases with less to specify.
- **Validating on read automatically.** Rejected: it makes old data unreadable when rules
  tighten. See [Reading old data](#reading-old-data).
- **Functions looked up by name at run time in Rust.** Rejected: a missing function would
  only be found at run time. The generated trait finds it at compile time.

## Decisions

- **Rules on list elements** are written as named functions on the list. There is no
  built-in `each(…)` form: it would add syntax for a case a one-line function covers.
- **Unknown enum values** can be rejected on `validate` with `known`.
- **No rules inside structs.** Structs are frozen, so a rule written inside one could never
  change. Rules go on the table fields that use the struct, usually as a named function
  (`#[check(non_negative_money)] total @0: common.Money;`), where each table chooses its
  rule and can evolve it.
- **One confirming attribute.** A tightened rule is confirmed with the existing
  `#[allow_breaking_readers]`. A single attribute means "I know this can affect data that
  already exists", whether the effect is on reading or on validation.

## Test plan

- Parser: attributes with arguments, several in one bracket, attributes on tables.
- Checker: each rule against each type, `known` only on enums, rules inside a struct
  refused, bounds that do not fit, `min > max`, a default that breaks its own rule, one
  name used with two input types.
- Compatibility corpus: a case for every row of the evolution table.
- Codegen: the `Checks` trait, `validate`, `to_bytes_validated`, the error's field and
  index for a failure inside a list of tables, and an optional field left out.
- Dynamic: a registry missing a name fails at construction.
- Tools: `inspect` reports named functions as not checked.
- The conformance vectors stay unchanged.
