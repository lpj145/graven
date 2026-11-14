# Binary layout, by example

This page shows how each kind of value looks on the wire. Every example below was encoded by
the reference implementation (`gravenc from-json`) when this site was built, then annotated
from the layout in the lockfile. Point at a byte or a label to see what it encodes.

The examples are bare messages, without the [envelope](spec/graven-format-v1.md#8-envelope).
Byte offsets start at 0. Pointers are relative to the position of the pointer itself, so a
message can be copied or embedded anywhere without fixing them up.

Five colours are used throughout: structure (lengths, counts, presence bits, union tags),
inline values, pointers, variable-size data, and absent fields.

## Scalars, enums and presence

```graven
enum Unit : u8 { Celsius = 0, Fahrenheit = 1 }

table Reading {
  device  @0: u32;
  temp    @1: f32?;
  battery @2: u8?;
  unit    @3: Unit;
  online  @4: bool;
}
```

A table starts with a `u16` giving the length of its fixed part. Scalars live in the fixed
part, each in a slot at a position the compiler computes once and records in the lockfile.
One presence bit per scalar tells a present zero from an absent field: here `battery` is
absent, so its slot is ignored. A `bool` is two bits in the presence byte and has no slot.

<figure class="anatomy layout" data-example="scalars"><p class="explain">The example needs JavaScript.</p></figure>

## Strings and bytes

```graven
table Note {
  title @0: string;
  body  @1: string?;
  tag   @2: bytes?;
}
```

Variable-size data lives after the fixed part. The slot holds a 4-byte relative pointer; a
zero pointer means absent. The data is a `u32` length followed by the bytes, which a reader
borrows straight from the buffer.

<figure class="anatomy layout" data-example="strings"><p class="explain">The example needs JavaScript.</p></figure>

## Lists of scalars and structs

```graven
struct Point { x: i16, y: i16 }

table Path {
  ids    @0: [u16];
  points @1: [Point];
}
```

A list of fixed-size values is a `u32` count followed by the elements, packed with no
padding. Element `i` is at `4 + i × size`, so indexing is a multiplication, not a scan.

<figure class="anatomy layout" data-example="lists"><p class="explain">The example needs JavaScript.</p></figure>

## Lists of tables

```graven
table Product {
  name  @0: string;
  price @1: u32;
}

table Catalog {
  products @0: [Product];
}
```

This is the shape of a JSON array of objects, such as
`{"products": [{"name": "pen", "price": 150}, …]}` (see
[Coming from JSON](coming-from-json.md)). The list starts with a `u32` count and a `u16`
element length. The elements' fixed parts follow back to back, without a header each, so
element `i` is at `6 + i × length`. Each element can still have its own strings, lists and
nested tables: their pointers lead to data after the list.

Storing the element length in the list, rather than taking it from the schema, is what lets
a list written by a newer schema (with longer elements) be read by an older one, and the
other way round.

<figure class="anatomy layout" data-example="tables"><p class="explain">The example needs JavaScript.</p></figure>

## Lists of strings

```graven
table Tags {
  tags @0: [string];
}
```

A list of strings or byte arrays is a `u32` count followed by one pointer per element.

<figure class="anatomy layout" data-example="strlist"><p class="explain">The example needs JavaScript.</p></figure>

## Nested tables

```graven
table Customer {
  name @0: string;
  home @1: Address?;
}

table Address {
  city @0: string;
  zip  @1: u32;
}
```

A table field is a pointer to another table, with its own length header and presence bits.
Each table evolves on its own: adding a field to `Address` does not change `Customer`.

<figure class="anatomy layout" data-example="nested"><p class="explain">The example needs JavaScript.</p></figure>

## Unions

```graven
table Card { last4 @0: u16; }
table Pix  { key @0: string; }

union Method { Card @1: Card, Pix @2: Pix }

table Payment {
  amount @0: u32;
  method @1: Method;
}
```

A union slot is a `u16` tag followed by a pointer to the variant's table. Tag 0 means no
value. A reader that does not know a tag reports it as unknown instead of failing, so new
variants can be added.

<figure class="anatomy layout" data-example="union"><p class="explain">The example needs JavaScript.</p></figure>

The normative rules for all of the above are in the
[specification](spec/graven-format-v1.md).

<script src="../assets/layouts.js" defer></script>
