import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  type Bit,
  type LeCodec,
  Builder,
  type TableW,
  leF32,
  leF64,
  leI16,
  leI128,
  leI64,
  leU128,
  leU16,
  leU32,
  leU64,
  leU8,
  schemaId,
} from "./gen/graven.js";
import { type LeafW, Root, RootW, RootV2W } from "./gen/conformance.js";

interface Point {
  readonly x: number;
  readonly y: number;
}

const POINT_CODEC: LeCodec<Point> = {
  size: 8,
  read: (dv, off) => ({ x: dv.getInt32(off, true), y: dv.getInt32(off + 4, true) }),
  write: (dv, off, v) => {
    dv.setInt32(off, v.x, true);
    dv.setInt32(off + 4, v.y, true);
  },
};

const b = (byte: number, shift: number): Bit => ({ byte, shift });

function writeLeaf(t: TableW, n: number, s?: string, writeNext?: (next: TableW) => void): void {
  t.set(1, b(0, 0), leU32, n);
  if (s !== undefined) t.setStr(5, s);
  if (writeNext !== undefined) writeNext(t.table(9, 13));
}

function buildMinimal(fixedLen = 112): Builder {
  const builder = new Builder();
  const root = builder.root(fixedLen);
  root.set(1, b(0, 0), leU8, 42);
  root.setBool(b(60, 2), true);
  return builder;
}

function buildAllFields(): Builder {
  const builder = new Builder();
  const r = builder.root(112);
  r.set(1, b(0, 0), leU8, 200);
  r.set(2, b(0, 1), leI16, -12345);
  r.set(4, b(0, 2), leU32, 4_000_000_000);
  r.set(8, b(0, 3), leI64, -9_000_000_000_000_000_000n);
  r.set(16, b(0, 4), leU128, 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210n);
  r.set(32, b(0, 5), leI128, -(1n << 127n));
  r.set(48, b(0, 6), leF32, 1.5);
  r.set(52, b(0, 7), leF64, -1234.5678);
  r.setBool(b(60, 0), true);
  r.setBool(b(60, 2), true);
  r.set(61, b(60, 4), leU8, 2);
  r.set(62, b(60, 5), POINT_CODEC, { x: -1, y: 0x7fff_ffff });
  r.setStr(70, "grvn ✓ ação");
  r.setBytes(74, new Uint8Array([0x00, 0x01, 0xfe, 0xff]));
  r.setScalars(78, leU16, [0, 1, 65535]);
  r.setScalars(82, POINT_CODEC, [
    { x: 1, y: 2 },
    { x: -3, y: -4 },
  ]);
  const leaves = r.tables(86, 3, 13);
  writeLeaf(leaves.push(), 1, "a");
  writeLeaf(leaves.push(), 2);
  writeLeaf(leaves.push(), 3, "c", (next) => writeLeaf(next, 4, "d"));
  leaves.close();

  r.setStrs(90, ["", "x", "olá"]);
  r.setBytesList(94, [
    new Uint8Array([]),
    new Uint8Array([0xab]),
    new Uint8Array([1, 2, 3]),
  ]);
  writeLeaf(r.table(98, 13), 10, "filho", (next11) =>
    writeLeaf(next11, 11, undefined, (next12) => writeLeaf(next12, 12)),
  );
  writeLeaf(r.union(102, 1, 13), 20, "escolha");
  return builder;
}

const HERE = path.dirname(fileURLToPath(import.meta.url));
const VECTORS_DIR = path.resolve(HERE, "../../../testvectors/v1");

function vectorBytes(name: string): Uint8Array {
  const b = fs.readFileSync(path.join(VECTORS_DIR, `${name}.bin`));
  return new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
}

describe("canonical writer parity (byte-for-byte against testvectors/v1)", () => {
  it("minimal", () => {
    assert.deepEqual(buildMinimal().finish(), vectorBytes("minimal"));
  });

  it("all_fields", () => {
    assert.deepEqual(buildAllFields().finish(), vectorBytes("all_fields"));
  });

  it("enveloped_all_fields", () => {
    const id = schemaId("conformance.v1.Root");
    assert.deepEqual(buildAllFields().finishEnvelope(id), vectorBytes("enveloped_all_fields"));
  });

  it("zero_is_present", () => {
    const builder = new Builder();
    const r = builder.root(112);
    r.set(1, b(0, 0), leU8, 0);
    r.set(2, b(0, 1), leI16, 0);
    r.set(4, b(0, 2), leU32, 0);
    r.set(8, b(0, 3), leI64, 0n);
    r.set(16, b(0, 4), leU128, 0n);
    r.set(32, b(0, 5), leI128, 0n);
    r.set(48, b(0, 6), leF32, 0.0);
    r.set(52, b(0, 7), leF64, 0.0);
    r.setBool(b(60, 0), false);
    r.setBool(b(60, 2), false);
    r.set(61, b(60, 4), leU8, 0);
    r.set(62, b(60, 5), POINT_CODEC, { x: 0, y: 0 });
    assert.deepEqual(builder.finish(), vectorBytes("zero_is_present"));
  });

  it("empty_lists", () => {
    const builder = buildMinimal();
    const r = new Builder();
    void builder;
    const root = r.root(112);
    root.set(1, b(0, 0), leU8, 42);
    root.setBool(b(60, 2), true);
    root.setStr(70, "");
    root.setBytes(74, new Uint8Array(0));
    root.setScalars(78, leU16, []);
    root.setScalars(82, POINT_CODEC, []);
    root.tables(86, 0, 13).close();
    root.setStrs(90, []);
    root.setBytesList(94, []);
    assert.deepEqual(r.finish(), vectorBytes("empty_lists"));
  });

  it("streaming_leaves", () => {
    const builder = new Builder();
    const r = builder.root(112);
    r.set(1, b(0, 0), leU8, 42);
    r.setBool(b(60, 2), true);
    const l = r.tablesStreaming(86, 13, [5, 9]);
    writeLeaf(l.push(), 1, "um");
    writeLeaf(l.push(), 2, "dois");
    writeLeaf(l.push(), 3, undefined, (next) => writeLeaf(next, 4, "quatro"));
    l.close();
    assert.deepEqual(builder.finish(), vectorBytes("streaming_leaves"));
  });

  it("v1_read_by_v2 and v2_read_by_v1", () => {
    const b1 = new Builder();
    const r1 = b1.root(112);
    r1.set(1, b(0, 0), leU8, 42);
    r1.setBool(b(60, 2), true);
    r1.setStr(70, "v1");
    assert.deepEqual(b1.finish(), vectorBytes("v1_read_by_v2"));

    const b2 = new Builder();
    const r2 = b2.root(120);
    r2.set(1, b(0, 0), leU8, 42);
    r2.setBool(b(60, 2), true);
    r2.setStr(70, "v1");
    r2.set(112, b(60, 7), leU64, 0x1122_3344_5566_7788n);
    assert.deepEqual(b2.finish(), vectorBytes("v2_read_by_v1"));
  });

  it("scalarsStreaming and batch sealing", () => {
    const builder = new Builder();
    const r1 = builder.root(112);
    r1.set(1, b(0, 0), leU8, 1);
    r1.setBool(b(60, 2), true);
    const s = r1.scalarsStreaming(78, leU16);
    s.push(10).push(20).push(30);
    s.close();
    const id = schemaId("conformance.v1.Root");
    builder.sealEnvelope(id);

    const r2 = builder.root(112);
    r2.set(1, b(0, 0), leU8, 2);
    r2.setBool(b(60, 2), false);
    builder.sealEnvelope(id);

    const batch = builder.takeFrames();
    assert.ok(batch.length > 48);
  });
});

// The same canonical vectors, written through the generated writers (`RootW`, `LeafW`):
// a wrong offset or bit in the generator fails here, not only in the runtime.

function genLeaf(w: LeafW, n: number, s?: string, next?: (w: LeafW) => void): void {
  w.n(n);
  if (s !== undefined) w.s(s);
  if (next !== undefined) next(w.next());
}

function genAllFields(w: RootW): void {
  w.a_u8(200).a_i16(-12345).a_u32(4_000_000_000).a_i64(-9_000_000_000_000_000_000n);
  w.a_u128(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210n).a_i128(-(1n << 127n));
  w.a_f32(1.5).a_f64(-1234.5678).flag(true).flag2(true).color(2);
  w.point({ x: -1, y: 0x7fff_ffff });
  w.name("grvn ✓ ação").blob(new Uint8Array([0x00, 0x01, 0xfe, 0xff]));
  w.nums([0, 1, 65535]).points([{ x: 1, y: 2 }, { x: -3, y: -4 }]);
  const leaves = w.leaves(3);
  genLeaf(leaves.push(), 1, "a");
  genLeaf(leaves.push(), 2);
  genLeaf(leaves.push(), 3, "c", (n) => genLeaf(n, 4, "d"));
  leaves.close();
  w.names(["", "x", "olá"]);
  w.blobs([new Uint8Array([]), new Uint8Array([0xab]), new Uint8Array([1, 2, 3])]);
  genLeaf(w.child(), 10, "filho", (n11) => genLeaf(n11, 11, undefined, (n12) => genLeaf(n12, 12)));
  genLeaf(w.choice_Leaf(), 20, "escolha");
}

function bare(fn: (w: RootW) => void): Uint8Array {
  const builder = new Builder();
  fn(RootW.root(builder));
  return builder.finish();
}

describe("generated writers produce the canonical vectors byte for byte", () => {
  it("minimal", () => {
    assert.deepEqual(bare((w) => w.a_u8(42).flag2(true)), vectorBytes("minimal"));
  });

  it("all_fields and enveloped_all_fields", () => {
    assert.deepEqual(bare(genAllFields), vectorBytes("all_fields"));
    assert.deepEqual(RootW.toBytes(genAllFields), vectorBytes("enveloped_all_fields"));
  });

  it("zero_is_present", () => {
    const bytes = bare((w) => {
      w.a_u8(0).a_i16(0).a_u32(0).a_i64(0n).a_u128(0n).a_i128(0n).a_f32(0).a_f64(0);
      w.flag(false).flag2(false).color(0).point({ x: 0, y: 0 });
    });
    assert.deepEqual(bytes, vectorBytes("zero_is_present"));
  });

  it("empty_lists", () => {
    const bytes = bare((w) => {
      w.a_u8(42).flag2(true).name("").blob(new Uint8Array(0)).nums([]).points([]);
      w.leaves(0).close();
      w.names([]).blobs([]);
    });
    assert.deepEqual(bytes, vectorBytes("empty_lists"));
  });

  it("v1_read_by_v2 and v2_read_by_v1", () => {
    assert.deepEqual(bare((w) => w.a_u8(42).flag2(true).name("v1")), vectorBytes("v1_read_by_v2"));
    const builder = new Builder();
    RootV2W.root(builder).a_u8(42).flag2(true).name("v1").extra(0x1122_3344_5566_7788n);
    assert.deepEqual(builder.finish(), vectorBytes("v2_read_by_v1"));
  });

  it("streaming_leaves", () => {
    const bytes = bare((w) => {
      w.a_u8(42).flag2(true);
      const l = w.leaves_streaming();
      genLeaf(l.push(), 1, "um");
      genLeaf(l.push(), 2, "dois");
      genLeaf(l.push(), 3, undefined, (n) => genLeaf(n, 4, "quatro"));
      l.close();
    });
    assert.deepEqual(bytes, vectorBytes("streaming_leaves"));
  });
});

describe("streaming scalar list writer", () => {
  it("refuses a push after another variable-size field was written in between", () => {
    assert.throws(
      () =>
        RootW.toBytes((r) => {
          const nums = r.nums_streaming();
          nums.push(1);
          r.name("hello");
          nums.push(2);
        }),
      /streaming list/,
    );
  });

  it("allows other fields before the list and after its last push", () => {
    const bytes = RootW.toBytes((r) => {
      r.a_u8(1).a_u32(2).flag(true).flag2(false).name("before");
      const nums = r.nums_streaming();
      nums.push(1).push(2);
      r.blob(new Uint8Array([9]));
    });
    const v = Root.fromBytes(bytes);
    assert.deepEqual([...v.nums()!], [1, 2]);
    assert.equal(v.name(), "before");
  });
});
