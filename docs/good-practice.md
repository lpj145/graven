# Good practice

Advice for designing schemas and using them well. None of it is required by the format. Each
tip gives its reason, so you can tell when it does not apply.

## Designing schemas

### Make a field required only if it can never go away

A required field is a promise to every reader, including ones already deployed. Once it
exists, no writer can stop sending it without breaking those readers: the compiler stops
you with the `required-reserved` or `required-relaxed` warning until you confirm with
`#[allow_breaking_readers]`.

Use required for what defines the record: an id, a timestamp, an amount. Make everything
else optional (`?`) or give it a default.

```graven
table Order {
  id       @0: common.Uuid;     // the record is meaningless without it
  total    @1: common.Money;
  coupon   @2: string?;         // may be dropped some day
  priority @3: u8 = 1;          // absent reads as 1
}
```

### Pick a default only when you will never change it

Changing a default is refused (`default-changed`). Old messages that left the field out
would suddenly read a different value. If the "normal" value may change later, make the
field optional and apply the fallback in your code.

### Never delete a field: reserve it

When a field is no longer used, replace it with `@N: reserved;`. Its ID and its bytes in
the layout stay taken, so a future field can never be misread as the old one. Renaming a
field is free: the name is not on the wire.

### Make incompatible changes as a new type

If a change cannot be made compatibly, declare a new table (`OrderV2`) next to the old one.
It gets its own schema id, the two can coexist, and readers choose which one to accept. Do
not put versions in namespaces.

### Use enums for closed sets of values

A field like `"status": "Active"` stored as a string costs 4 bytes of pointer, 4 of length
and the text. As an enum it costs 1 byte and a presence bit, and a typo cannot get in.

```graven
enum Status : u8 { Draft = 0, Active = 1, Archived = 2 }
```

Readers must handle values they do not know: a newer writer may add a variant, and the
generated code reports it as `Unknown(n)` instead of failing.

### Use structs only for values that will never change

A struct is stored inline, with no pointer and no presence bits per member, which makes it
the most compact way to group fields. In exchange it is frozen: any change is refused
(`struct-changed`). Good structs: coordinates, a UUID, an amount with its currency. If you
can imagine adding a member, use a table.

### Choose integer sizes for the range, not for the typical value

Integers are stored at their full width, with no variable-length encoding, so a `u64`
always takes 8 bytes. Use `u8`/`u16` when the range is truly bounded (a percentage, a port).
Use `u64` for identifiers and counters that grow. Changing a type later is refused
(`type-changed`), so leave room.

### Store money as integers

Use an integer amount of minor units with a currency code, never `f32`/`f64`, which cannot
represent most decimal fractions exactly.

```graven
struct Money { cents: i64, currency: u16 }   // currency: ISO 4217 numeric code
```

### Share common types through imports

Put types used across schemas (`Uuid`, `Money`, `Address`) in one file and
[import](graven-language.md#7-imports) it. Copies in each file drift apart, and nothing
checks that they stay identical.

### Document fields with `///`

[Doc comments](graven-language.md#documentation-comments) are carried into the generated
code. Units, valid ranges and the meaning of an absent field are worth writing down: the
schema outlives the people who wrote it.

## Lists and collections

### Wrap a list that travels as a unit in a table

An API response, a configuration or a snapshot should be one message that holds a list.
The whole list then costs 12 bytes of structure plus about 8 bytes per string element,
with a single envelope if you use one. A top-level JSON array such as
`[{"name": "product_a"}, …]` with 100 elements takes 1712 bytes this way, against 2101 as
compact JSON.

### Store growing collections as one message per record

For logs, events and exports, write one enveloped message per record. Records are appended
without rewriting the file, each one is checked on its own, and
[`gravenc scan`](tools.md#scan) tells a crash-truncated tail from real corruption. The
price is 24 bytes of envelope per record: for 100 tiny records, 4300 bytes against 1712 for
one wrapped list. Do not use this layout for small lists that are always read whole.

### Prefer lists of structs for small fixed records

A `[Point]` stores its elements back to back with no pointers. A list of tables costs a
pointer per string or nested value inside each element, but its elements can evolve. Use
structs for numeric samples, coordinates and similar data that will never gain a field.

### Give the length when you know it

Writing a list whose length you pass up front is faster than writing it as a stream (about
1.4–1.7× in the [benchmarks](benchmarks.md)). Stream only when the length is truly unknown,
for example while reading from an iterator.

## Reading and writing

### Verify untrusted bytes once, then read freely

Bytes from the network, from disk or from another process are untrusted. Open them with
`from_bytes` (enveloped) or `verified` (bare). This walks the message once under fixed
depth and work limits, and gives you a verified view whose reads do not check again.
Plain `read` is also safe on any input, but it checks on every access, so prefer it only
when you read a field or two.

### Read lazily

A message is not decoded up front: reading one field costs about a nanosecond, whatever
the size of the message. Pass the view around and read fields where you need them instead
of copying everything into your own structs.

### Use the envelope at boundaries

Files, queues and network connections between services should carry enveloped messages:
the schema id stops a message being read as the wrong type, and the CRC32C separates a cut
write from corruption. Inside one process, or when a transport already frames and checks
messages and the type is known, a bare message saves 24 bytes and the CRC (about 10 ns on a
small message, about 1.6 µs on 64 KiB).

### Reuse the builder

Keep one `Builder` per thread and reuse it between messages. After warm-up, writing a
message then allocates nothing.

## Process

### Commit the lockfiles and check them in CI

The lockfile is the memory of every layout you have shipped. Commit it next to the
schema, review its diff like code, and run `gravenc check --locked` in CI so that no build
uses a schema the lock has not accepted.

### Change the lock deliberately

Only `gravenc update` writes the lock, and `build.rs` never does. When `update` refuses a
change, read the rule it names before reaching for `#[allow_breaking_readers]`. The
attribute exists for breaks you have planned, not to silence the compiler.
