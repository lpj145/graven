// Proposal 0006: rebuild (`XW.rebuild`, `fillFrom`), `#[align(to = N)]`, record alignment and
// `ScalarList.asTypedArray`.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Builder, decodeEnvelope, envelopes } from "./gen/graven.js";
import { Root, RootW } from "./gen/conformance.js";
import { Batch, BatchW, Samples, SamplesW } from "./gen/aligned.js";
import { type JsonVal, readCase } from "./conformance_json.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const VECTORS_DIR = path.resolve(HERE, "../../../testvectors/v1");

/** Valid vectors a typed writer cannot produce (same list as the Rust test). */
const NOT_TYPED_WRITABLE = new Set([
  "unknown_union_tag",
  "v2_read_by_v1",
  "reserved_ignored",
  "bool_struct_and_lists",
  "strlist_null_elem",
  "union_tag_null_ptr_optional",
  "reserved_slot_partial",
  "zero_size_slot_at_end",
  "table_list_elem_wider",
  "table_list_elem_narrower",
  "env_concat",
  "streaming_leaves",
]);

function rebuildUnchanged(bytes: Uint8Array, enveloped: boolean): Uint8Array {
  if (enveloped) return RootW.rebuild(Root.fromBytes(bytes), () => {});
  const b = new Builder();
  RootW.root(b).fillFrom(Root.verified(bytes));
  return b.finish();
}

/** `bytes` copied to an offset of `buf` that is a multiple of 4096. */
function pageAligned(bytes: Uint8Array): Uint8Array {
  const buf = new Uint8Array(new ArrayBuffer(bytes.length + 4096), 4096);
  buf.set(bytes);
  return buf;
}

const offsetIn = (whole: Uint8Array, part: Uint8Array): number => part.byteOffset - whole.byteOffset;

describe("rebuild (proposal 0006)", () => {
  it("rebuilding a vector without changes reproduces it, or reads the same", () => {
    let exact = 0;
    for (const file of fs.readdirSync(VECTORS_DIR).filter((f) => f.endsWith(".json"))) {
      const name = file.slice(0, -5);
      const spec = JSON.parse(fs.readFileSync(path.join(VECTORS_DIR, file), "utf8")) as {
        schema: string;
        enveloped: boolean;
        expect: { value?: JsonVal };
      };
      if (spec.expect.value === undefined || spec.schema !== "conformance.v1.Root" || name === "env_concat") continue;
      const bin = new Uint8Array(fs.readFileSync(path.join(VECTORS_DIR, `${name}.bin`)));
      const got = rebuildUnchanged(bin, spec.enveloped);
      if (name === "unknown_union_tag") {
        assert.equal(Root.verified(got).choice(), undefined, "an unknown variant is not copied");
      } else if (NOT_TYPED_WRITABLE.has(name)) {
        assert.deepEqual(readCase(spec.schema, spec.enveloped, got), spec.expect.value, name);
      } else {
        assert.deepEqual(got, bin, `${name}: rebuild does not reproduce the .bin`);
        exact++;
      }
    }
    assert.ok(exact >= 6, `only ${exact} vectors reproduced exactly`);
  });

  it("changes only what the closure writes", () => {
    const old = RootW.toBytes((w) => {
      w.a_u8(1).flag2(true).name("old name").nums([1, 2, 3]).blob(new Uint8Array([1, 2]));
      w.names(["a", "b"]);
      w.child().n(9).s("kept");
      const l = w.leaves(2);
      l.push().n(1);
      l.push().n(2).s("two");
      w.choice_Leaf().n(77);
    });
    const ov = Root.fromBytes(old);
    const neu = RootW.rebuild(ov, (w) => {
      w.name("new name");
      w.child().s("changed").fillFrom(ov.child()!);
    });
    const nv = Root.fromBytes(neu);
    assert.equal(nv.name(), "new name");
    assert.equal(nv.child()!.s(), "changed");
    assert.equal(nv.child()!.n(), 9);
    const strip = (v: JsonVal): JsonVal => {
      const t = { ...(v as { Table: Record<string, JsonVal> }).Table };
      delete t.name;
      delete t.child;
      return { Table: t };
    };
    assert.deepEqual(strip(readCase("conformance.v1.Root", true, neu)), strip(readCase("conformance.v1.Root", true, old)));
    // `a_u32` (default 7) was absent: it stays absent.
    const payload = decodeEnvelope(neu).payload;
    assert.equal(Root.read(payload)._ref.bit({ byte: 0, shift: 2 }), false);
  });
});

describe("#[align(to = N)] (proposal 0006)", () => {
  const write = (b?: Builder) => {
    const fn = (w: SamplesW) => {
      w.name("odd-length name").values([1, 2.5, -3]).ids([7n, 8n]).blob(new Uint8Array([1, 2, 3]));
      w.page(new Uint8Array([1, 2, 3, 4]));
    };
    return b === undefined ? SamplesW.toBytes(fn) : SamplesW.toBytesWith(b, fn);
  };

  it("aligns lists and bytes from the envelope start", () => {
    const env = pageAligned(write());
    const s = Samples.fromBytes(env);
    assert.equal(offsetIn(env, s.values().asBytes()) % 16, 0);
    assert.deepEqual(s.values().asTypedArray(), new Float32Array([1, 2.5, -3]));
    assert.ok(s.values().asTypedArray() instanceof Float32Array);
    assert.equal(offsetIn(env, s.ids()!.asBytes()) % 8, 0);
    assert.deepEqual(s.ids()!.asTypedArray(), new BigUint64Array([7n, 8n]));
    assert.equal(offsetIn(env, s.blob()!) % 64, 0);
    assert.equal(offsetIn(env, s.page()!.asBytes()) % 4096, 0);
    assert.equal(s.name(), "odd-length name");
  });

  it("writes the same bytes as the Rust writer (pinned in crates/graven-codegen-tests/tests/aligned.rs)", () => {
    const env = SamplesW.toBytes((w) => {
      w.name("odd-length name").values([1, 2.5, -3]).ids([7n, 8n]).blob(new TextEncoder().encode("abc"));
    });
    const hex = Array.from(env, (x) => x.toString(16).padStart(2, "0")).join("");
    assert.equal(hex, "4752564e01000000fefb695008dd59a16f000000140014000000320000003e0000005a000000000000000f0000006f64642d6c656e677468206e616d65000000000000000000000000000000030000000000803f00002040000040c002000000070000000000000008000000000000000000000000000000000000000300000061626380282d5d");
    // The layout does not depend on the buffer it was written into.
    const b = new Builder();
    assert.deepEqual(write(b), write(b));
  });

  it("asTypedArray falls back when the elements are not aligned", () => {
    const env = write();
    const shifted = new Uint8Array(new ArrayBuffer(env.length + 1), 1);
    shifted.set(env);
    const s = Samples.fromBytes(shifted);
    assert.equal(s.values().asTypedArray(), undefined);
    assert.deepEqual([...s.values()], [1, 2.5, -3]);
  });

  it("aligns lists inside list elements, known-count and streamed", () => {
    for (const streaming of [false, true]) {
      const env = pageAligned(
        BatchW.toBytes((w) => {
          const l = streaming ? w.items_streaming() : w.items(3);
          for (let i = 0; i < 3; i++) l.push().name("n".repeat(i + 1)).values([i, i, i]);
          l.close();
        }),
      );
      let i = 0;
      for (const s of Batch.fromBytes(env).items()) {
        assert.equal(offsetIn(env, s.values().asBytes()) % 16, 0, `streaming=${streaming} item ${i}`);
        assert.deepEqual(s.values().asTypedArray(), new Float32Array([i, i, i]));
        i++;
      }
      assert.equal(i, 3);
    }
  });

  it("rebuild keeps the alignment", () => {
    const old = Samples.fromBytes(write());
    const neu = pageAligned(SamplesW.rebuild(old, (w) => w.name("a much longer name, to move what follows")));
    const s = Samples.fromBytes(neu);
    assert.equal(offsetIn(neu, s.values().asBytes()) % 16, 0);
    assert.equal(offsetIn(neu, s.blob()!) % 64, 0);
    assert.deepEqual(s.values().asTypedArray(), new Float32Array([1, 2.5, -3]));
  });
});

describe("record alignment (proposal 0006)", () => {
  it("pads every envelope to a multiple of the record size", () => {
    const b = new Builder().withRecordAlign(64);
    for (const n of [1, 50, 63, 64, 65, 200]) {
      const env = SamplesW.toBytesWith(b, (w) => w.name("x".repeat(n)).values([0.5]));
      assert.equal(env.length % 64, 0, `name of ${n}: ${env.length} bytes`);
      assert.equal(Samples.fromBytes(env).name().length, n);
    }
  });

  it("sealed envelopes all start at a multiple of the record size", () => {
    const b = new Builder().withRecordAlign(4096);
    for (let n = 0; n < 5; n++) SamplesW.seal(b, (w) => w.name("y".repeat(n * 700)).values([n]));
    const file = b.takeFrames();
    let at = 0;
    let count = 0;
    for (const e of envelopes(file)) {
      assert.equal(at % 4096, 0);
      assert.equal(Samples.verified(e.payload).values().get(0), count);
      at += e.totalLen;
      count++;
    }
    assert.equal(count, 5);
    assert.equal(at, file.length);
  });

  it("rejects a record size that is not a power of two", () => {
    assert.throws(() => new Builder().withRecordAlign(100), RangeError);
    assert.throws(() => new Builder().withRecordAlign(1), RangeError);
  });
});
