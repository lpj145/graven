// Reads a conformance vector with the generated code and converts the result to the JSON value
// of testvectors/v1/*.json. Shared by the conformance test and the fuzzer (fuzz.ts).
import { openEnvelope, schemaId } from "./gen/graven.js";
import {
  type ChoiceVerified,
  Extra,
  type ExtraVerified,
  Holder,
  type HolderVerified,
  type LeafVerified,
  Root,
  RootV2,
  type RootV2Verified,
  type RootVerified,
} from "./gen/conformance.js";

// ---- JSON value conversion matching testvectors/v1/*.json ----

const f32Buf = new ArrayBuffer(4);
const f32Dv = new DataView(f32Buf);
const f64Buf = new ArrayBuffer(8);
const f64Dv = new DataView(f64Buf);

function fmtF32(v: number): string {
  f32Dv.setFloat32(0, v, true);
  return `f32:0x${f32Dv.getUint32(0, true).toString(16).padStart(8, "0")}`;
}

function fmtF64(v: number): string {
  f64Dv.setFloat64(0, v, true);
  return `f64:0x${f64Dv.getBigUint64(0, true).toString(16).padStart(16, "0")}`;
}

function toHex(b: Uint8Array): string {
  let out = "";
  for (let i = 0; i < b.length; i++) {
    out += b[i]!.toString(16).padStart(2, "0");
  }
  return out;
}

export type JsonVal = Record<string, unknown>;

function leafToJson(l: LeafVerified): JsonVal {
  const t: Record<string, JsonVal> = {};
  t.n = { UInt: String(l.n()) };
  const s = l.s();
  if (s !== undefined) t.s = { Str: s };
  const next = l.next();
  if (next !== undefined) t.next = leafToJson(next);
  return { Table: t };
}

function choiceToJson(c: ChoiceVerified): JsonVal {
  if (c.variant === "Unknown") {
    return { Union: { tag: c.tag, value: { Table: {} } } };
  }
  return { Union: { tag: c.tag, value: leafToJson(c.value) } };
}

function rootFieldsToJson(r: RootVerified | RootV2Verified): Record<string, JsonVal> {
  const t: Record<string, JsonVal> = {};
  t.a_u8 = { UInt: String(r.a_u8()) };
  const a_i16 = r.a_i16();
  if (a_i16 !== undefined) t.a_i16 = { Int: String(a_i16) };
  t.a_u32 = { UInt: String(r.a_u32()) };
  const a_i64 = r.a_i64();
  if (a_i64 !== undefined) t.a_i64 = { Int: String(a_i64) };
  const a_u128 = r.a_u128();
  if (a_u128 !== undefined) t.a_u128 = { UInt: String(a_u128) };
  const a_i128 = r.a_i128();
  if (a_i128 !== undefined) t.a_i128 = { Int: String(a_i128) };
  const a_f32 = r.a_f32();
  if (a_f32 !== undefined) t.a_f32 = { F32: fmtF32(a_f32) };
  const a_f64 = r.a_f64();
  if (a_f64 !== undefined) t.a_f64 = { F64: fmtF64(a_f64) };
  const flag = r.flag();
  if (flag !== undefined) t.flag = { Bool: flag };
  t.flag2 = { Bool: r.flag2() };
  const color = r.color();
  if (color !== undefined) t.color = { UInt: String(color) };
  const pt = r.point();
  if (pt !== undefined) {
    t.point = { Table: { x: { Int: String(pt.x) }, y: { Int: String(pt.y) } } };
  }
  const name = r.name();
  if (name !== undefined) t.name = { Str: name };
  const blob = r.blob();
  if (blob !== undefined) t.blob = { Bytes: toHex(blob) };
  const nums = r.nums();
  if (nums !== undefined) t.nums = { List: [...nums].map((x) => ({ UInt: String(x) })) };
  const points = r.points();
  if (points !== undefined) {
    t.points = {
      List: [...points].map((p) => ({
        Table: { x: { Int: String(p.x) }, y: { Int: String(p.y) } },
      })),
    };
  }
  const leaves = r.leaves();
  if (leaves !== undefined) {
    t.leaves = { List: [...leaves].map((l) => leafToJson(l)) };
  }
  const names = r.names();
  if (names !== undefined) t.names = { List: [...names].map((s) => ({ Str: s })) };
  const blobs = r.blobs();
  if (blobs !== undefined) {
    t.blobs = { List: [...blobs].map((b) => ({ Bytes: toHex(b) })) };
  }
  const child = r.child();
  if (child !== undefined) t.child = leafToJson(child);
  const choice = r.choice();
  if (choice !== undefined) t.choice = choiceToJson(choice);
  return t;
}

function extraToJson(e: ExtraVerified): JsonVal {
  const t: Record<string, JsonVal> = {};
  const flags = e.flags();
  if (flags !== undefined) {
    t.flags = { Table: { c: { UInt: String(flags.c) }, on: { Bool: flags.on } } };
  }
  const bools = e.bools();
  if (bools !== undefined) t.bools = { List: [...bools].map((x) => ({ Bool: x })) };
  const colors = e.colors();
  if (colors !== undefined) t.colors = { List: [...colors].map((x) => ({ UInt: String(x) })) };
  const names = e.names();
  if (names !== undefined) t.names = { List: [...names].map((s) => ({ Str: s })) };
  const blobs = e.blobs();
  if (blobs !== undefined) {
    t.blobs = { List: [...blobs].map((b) => ({ Bytes: toHex(b) })) };
  }
  t.pick = choiceToJson(e.pick());
  const leaves = e.leaves();
  if (leaves !== undefined) {
    t.leaves = { List: [...leaves].map((l) => leafToJson(l)) };
  }
  const empty = e.e();
  if (empty !== undefined) t.e = { Table: {} };
  const tail = e.tail();
  if (tail !== undefined) t.tail = { Str: tail };
  return { Table: t };
}

function holderToJson(h: HolderVerified): JsonVal {
  const list = h.items();
  return {
    Table: {
      items: { List: [...list].map((l) => leafToJson(l)) },
    },
  };
}

export function readCase(schema: string, enveloped: boolean, bytes: Uint8Array): JsonVal {
  if (schema === "conformance.v1.Root") {
    const r = enveloped ? Root.fromBytes(bytes) : Root.verified(bytes);
    return { Table: rootFieldsToJson(r) };
  }
  if (schema === "conformance.v1.RootV2") {
    const payload = enveloped ? openEnvelope(bytes, RootV2.SCHEMA_ID) : bytes;
    const r = RootV2.verified(payload);
    const fields = rootFieldsToJson(r);
    const extra = r.extra();
    if (extra !== undefined) fields.extra = { UInt: String(extra) };
    return { Table: fields };
  }
  if (schema === "conformance.v1.Extra") {
    const payload = enveloped ? openEnvelope(bytes, schemaId(schema)) : bytes;
    return extraToJson(Extra.verified(payload));
  }
  if (schema === "conformance.v1.Holder") {
    const payload = enveloped ? openEnvelope(bytes, schemaId(schema)) : bytes;
    return holderToJson(Holder.verified(payload));
  }
  throw new Error(`unknown schema ${schema}`);
}

