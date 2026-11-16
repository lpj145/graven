/**
 * Graven runtime for JavaScript (format v1 and RPC frames v1).
 *
 * No dependencies, ES2022, for browsers, Node (>= 20) and Deno. This file is the source:
 * `scripts/ts.sh` compiles it with `tsc` into `graven.js` and `graven.d.ts` next to it, which
 * `gravenc gen --lang js` writes into projects.
 */

// ---- Errors ----

export type GravenErrorKind =
  | "OutOfBounds"
  | "Truncated"
  | "BadOffset"
  | "InvalidUtf8"
  | "Missing"
  | "DepthExceeded"
  | "BudgetExceeded"
  | "TooLarge"
  | "BadMagic"
  | "UnsupportedVersion"
  | "BadHeader"
  | "Incomplete"
  | "SchemaMismatch"
  | "ChecksumMismatch"
  | "UnsupportedFrameVersion"
  | "BadKind"
  | "BadFrameHeader";

export interface GravenErrorDetails {
  readonly field?: string;
  readonly needed?: number;
  readonly expected?: bigint | number;
  readonly found?: bigint | number;
}

/** Error thrown when decoding, verifying, or building a Graven message fails. */
export class GravenError extends Error {
  readonly kind: GravenErrorKind;
  readonly field?: string;
  readonly needed?: number;
  readonly expected?: bigint | number;
  readonly found?: bigint | number;

  constructor(kind: GravenErrorKind, details?: GravenErrorDetails, message?: string) {
    super(message ?? formatErrorMessage(kind, details));
    this.name = "GravenError";
    this.kind = kind;
    if (details?.field !== undefined) this.field = details.field;
    if (details?.needed !== undefined) this.needed = details.needed;
    if (details?.expected !== undefined) this.expected = details.expected;
    if (details?.found !== undefined) this.found = details.found;
  }
}

function formatErrorMessage(kind: GravenErrorKind, details?: GravenErrorDetails): string {
  if (!details) return kind;
  if (details.field !== undefined) return `${kind}(${details.field})`;
  if (details.needed !== undefined) return `${kind}(needed=${details.needed})`;
  if (details.expected !== undefined && details.found !== undefined) {
    return `${kind}(expected=${String(details.expected)}, found=${String(details.found)})`;
  }
  if (details.found !== undefined) return `${kind}(${String(details.found)})`;
  return kind;
}

// ---- Little-endian codecs & UTF-8 ----

const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const UTF8_DECODER_LOOSE = new TextDecoder("utf-8", { fatal: false, ignoreBOM: true });
export const EMPTY_BYTES = new Uint8Array(0);

export interface LeReader<T> {
  readonly size: number;
  read(dv: DataView, offset: number): T;
}

export interface LeCodec<T> extends LeReader<T> {
  write(dv: DataView, offset: number, value: T): void;
}

export const leU8: LeCodec<number> = {
  size: 1,
  read: (dv, off) => dv.getUint8(off),
  write: (dv, off, v) => dv.setUint8(off, v),
};

export const leU16: LeCodec<number> = {
  size: 2,
  read: (dv, off) => dv.getUint16(off, true),
  write: (dv, off, v) => dv.setUint16(off, v, true),
};

export const leU32: LeCodec<number> = {
  size: 4,
  read: (dv, off) => dv.getUint32(off, true),
  write: (dv, off, v) => dv.setUint32(off, v, true),
};

export const leU64: LeCodec<bigint> = {
  size: 8,
  read: (dv, off) => dv.getBigUint64(off, true),
  write: (dv, off, v) => dv.setBigUint64(off, v, true),
};

export const leU128: LeCodec<bigint> = {
  size: 16,
  read: (dv, off) => {
    const lo = dv.getBigUint64(off, true);
    const hi = dv.getBigUint64(off + 8, true);
    return (hi << 64n) | lo;
  },
  write: (dv, off, v) => {
    dv.setBigUint64(off, BigInt.asUintN(64, v), true);
    dv.setBigUint64(off + 8, BigInt.asUintN(64, v >> 64n), true);
  },
};

export const leI8: LeCodec<number> = {
  size: 1,
  read: (dv, off) => dv.getInt8(off),
  write: (dv, off, v) => dv.setInt8(off, v),
};

export const leI16: LeCodec<number> = {
  size: 2,
  read: (dv, off) => dv.getInt16(off, true),
  write: (dv, off, v) => dv.setInt16(off, v, true),
};

export const leI32: LeCodec<number> = {
  size: 4,
  read: (dv, off) => dv.getInt32(off, true),
  write: (dv, off, v) => dv.setInt32(off, v, true),
};

export const leI64: LeCodec<bigint> = {
  size: 8,
  read: (dv, off) => dv.getBigInt64(off, true),
  write: (dv, off, v) => dv.setBigInt64(off, v, true),
};

export const leI128: LeCodec<bigint> = {
  size: 16,
  read: (dv, off) => {
    const lo = dv.getBigUint64(off, true);
    const hi = dv.getBigUint64(off + 8, true);
    return BigInt.asIntN(128, (hi << 64n) | lo);
  },
  write: (dv, off, v) => {
    dv.setBigUint64(off, BigInt.asUintN(64, v), true);
    dv.setBigUint64(off + 8, BigInt.asUintN(64, v >> 64n), true);
  },
};

export const leF32: LeCodec<number> = {
  size: 4,
  read: (dv, off) => dv.getFloat32(off, true),
  write: (dv, off, v) => dv.setFloat32(off, v, true),
};

export const leF64: LeCodec<number> = {
  size: 8,
  read: (dv, off) => dv.getFloat64(off, true),
  write: (dv, off, v) => dv.setFloat64(off, v, true),
};

/** 1-byte boolean representation used in `[bool]` and inside `struct` definitions. */
export const leBool: LeCodec<boolean> = {
  size: 1,
  read: (dv, off) => dv.getUint8(off) !== 0,
  write: (dv, off, v) => dv.setUint8(off, v ? 1 : 0),
};

function viewOf(buf: Uint8Array): DataView {
  return new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
}

function readU16At(buf: Uint8Array, dv: DataView, pos: number): number {
  if (pos < 0 || pos + 2 > buf.length) {
    throw new GravenError("OutOfBounds", { needed: pos + 2 });
  }
  return dv.getUint16(pos, true);
}

function readU32At(buf: Uint8Array, dv: DataView, pos: number): number {
  if (pos < 0 || pos + 4 > buf.length) {
    throw new GravenError("OutOfBounds", { needed: pos + 4 });
  }
  return dv.getUint32(pos, true);
}

function resolvePtrAt(buf: Uint8Array, dv: DataView, slotAbs: number): number | undefined {
  const rel = dv.getInt32(slotAbs, true);
  if (rel === 0) return undefined;
  const target = slotAbs + rel;
  if (target < 0 || target >= buf.length) {
    throw new GravenError("BadOffset");
  }
  return target;
}

function bytesAt(buf: Uint8Array, dv: DataView, pos: number): Uint8Array {
  const len = readU32At(buf, dv, pos);
  const end = pos + 4 + len;
  if (end > buf.length) {
    throw new GravenError("OutOfBounds", { needed: end });
  }
  return buf.subarray(pos + 4, end);
}

function decodeUtf8(bytes: Uint8Array): string {
  try {
    return UTF8_DECODER.decode(bytes);
  } catch {
    throw new GravenError("InvalidUtf8");
  }
}

function countedAt(
  buf: Uint8Array,
  dv: DataView,
  pos: number,
  elemSize: number,
): { len: number; data: number } {
  const len = readU32At(buf, dv, pos);
  const data = pos + 4;
  const end = data + len * elemSize;
  if (end > buf.length) {
    throw new GravenError("OutOfBounds", { needed: end });
  }
  return { len, data };
}

// ---- Table & list readers ----

/** Bit position of a presence (or bool value) bit inside a table's fixed part. */
export interface Bit {
  readonly byte: number;
  readonly shift: number;
}

/** Low-level zero-copy cursor over a serialized Graven table. */
export class TableRef {
  readonly buf: Uint8Array;
  readonly dv: DataView;
  readonly start: number;
  readonly fixedLen: number;

  private constructor(buf: Uint8Array, dv: DataView, start: number, fixedLen: number) {
    this.buf = buf;
    this.dv = dv;
    this.start = start;
    this.fixedLen = fixedLen;
  }

  /** Table rooted at byte position `pos`: `[u16 fixed_len][fixed_part]`. */
  static at(buf: Uint8Array, pos = 0, dv?: DataView): TableRef {
    const view = dv ?? viewOf(buf);
    const fixedLen = readU16At(buf, view, pos);
    const start = pos + 2;
    if (start + fixedLen > buf.length) {
      throw new GravenError("OutOfBounds", { needed: start + fixedLen });
    }
    return new TableRef(buf, view, start, fixedLen);
  }

  /** Inline table element (no `u16` length prefix; length comes from the enclosing `TableList`). */
  static inline(buf: Uint8Array, start: number, fixedLen: number, dv?: DataView): TableRef {
    if (start < 0 || start + fixedLen > buf.length) {
      throw new GravenError("OutOfBounds", { needed: start + fixedLen });
    }
    return new TableRef(buf, dv ?? viewOf(buf), start, fixedLen);
  }

  /** Reads bit `b`, returning `false` when beyond `fixedLen` (older writer). */
  bit(b: Bit): boolean {
    if (b.byte >= this.fixedLen) return false;
    const byteVal = this.buf[this.start + b.byte];
    return byteVal !== undefined && ((byteVal >> b.shift) & 1) !== 0;
  }

  /** Raw byte at `byte` within `fixedLen` (caller guarantees `byte < fixedLen`). */
  fixedByte(byte: number): number {
    return this.buf[this.start + byte] ?? 0;
  }

  /**
   * Checks slot bounds relative to `fixedLen`:
   * - `true`: completely inside `fixedLen`
   * - `false`: starts at or beyond `fixedLen` (evolved older writer)
   * - throws `Truncated`: slot is partially covered by `fixedLen`
   */
  slotState(off: number, size: number): boolean {
    if (size === 0) return off <= this.fixedLen;
    if (off >= this.fixedLen) return false;
    if (off + size > this.fixedLen) {
      throw new GravenError("Truncated", { needed: off + size });
    }
    return true;
  }

  rawU16(off: number): number {
    if (off + 2 > this.fixedLen) return 0;
    return this.dv.getUint16(this.start + off, true);
  }

  rawI32(off: number): number {
    if (off + 4 > this.fixedLen) return 0;
    return this.dv.getInt32(this.start + off, true);
  }

  /** Reads a fixed-size slot without checking presence bits; returns `fallback` if out of `fixedLen`. */
  raw<T>(off: number, codec: LeReader<T>, fallback: T): T {
    if (off + codec.size > this.fixedLen) return fallback;
    return codec.read(this.dv, this.start + off);
  }

  /** Reads a `Scalar`, `Enum`, or `Struct` field; returns `undefined` when absent. */
  scalar<T>(off: number, presence: Bit, codec: LeReader<T>): T | undefined {
    if (!this.slotState(off, codec.size) || !this.bit(presence)) return undefined;
    return codec.read(this.dv, this.start + off);
  }

  /** Reads a fixed-size field, returning `defaultValue` when absent (even if slot is out of `fixedLen`). */
  scalarOr<T>(off: number, presence: Bit, codec: LeReader<T>, defaultValue: T): T {
    if (off + codec.size <= this.fixedLen && this.bit(presence)) {
      return codec.read(this.dv, this.start + off);
    }
    return defaultValue;
  }

  /** Reads a 2-bit table boolean field; returns `undefined` when absent. */
  bool(presence: Bit): boolean | undefined {
    if (!this.bit(presence)) return undefined;
    return this.bit({ byte: presence.byte, shift: presence.shift + 1 });
  }

  /** Resolves an `i32` relative pointer slot; returns `undefined` if outside `fixedLen` or `0`. */
  ptr(off: number): number | undefined {
    if (!this.slotState(off, 4)) return undefined;
    return resolvePtrAt(this.buf, this.dv, this.start + off);
  }

  bytes(off: number): Uint8Array | undefined {
    const pos = this.ptr(off);
    return pos === undefined ? undefined : bytesAt(this.buf, this.dv, pos);
  }

  str(off: number): string | undefined {
    const b = this.bytes(off);
    return b === undefined ? undefined : decodeUtf8(b);
  }

  /** Pre-verified UTF-8 read (for `*Verified` views after `verify()`). */
  strUnchecked(off: number): string | undefined {
    const b = this.bytes(off);
    return b === undefined ? undefined : UTF8_DECODER_LOOSE.decode(b);
  }

  table(off: number): TableRef | undefined {
    const pos = this.ptr(off);
    return pos === undefined ? undefined : TableRef.at(this.buf, pos, this.dv);
  }

  union(off: number): { readonly tag: number; readonly table: TableRef } | undefined {
    if (!this.slotState(off, 6)) return undefined;
    const tag = this.dv.getUint16(this.start + off, true);
    if (tag === 0) return undefined;
    const pos = resolvePtrAt(this.buf, this.dv, this.start + off + 2);
    if (pos === undefined) return undefined;
    return { tag, table: TableRef.at(this.buf, pos, this.dv) };
  }

  scalars<T>(off: number, codec: LeReader<T>): ScalarList<T> | undefined {
    const pos = this.ptr(off);
    if (pos === undefined) return undefined;
    const { len, data } = countedAt(this.buf, this.dv, pos, codec.size);
    return new ScalarList(this.buf, this.dv, data, len, codec);
  }

  tables(off: number): TableList | undefined {
    const pos = this.ptr(off);
    if (pos === undefined) return undefined;
    const len = readU32At(this.buf, this.dv, pos);
    const elemFixedLen = readU16At(this.buf, this.dv, pos + 4);
    const first = pos + 6;
    const end = first + len * elemFixedLen;
    if (end > this.buf.length) {
      throw new GravenError("OutOfBounds", { needed: end });
    }
    return new TableList(this.buf, this.dv, first, len, elemFixedLen);
  }

  strs(off: number): StrList | undefined {
    const pos = this.ptr(off);
    if (pos === undefined) return undefined;
    const { len, data } = countedAt(this.buf, this.dv, pos, 4);
    return new StrList(this.buf, this.dv, data, len);
  }

  bytesList(off: number): BytesList | undefined {
    const s = this.strs(off);
    return s === undefined ? undefined : new BytesList(s);
  }
}

/** View over a serialized `[T]` list of fixed-size scalars, bools, enums, or structs. */
export class ScalarList<T> implements Iterable<T> {
  private readonly buf: Uint8Array;
  private readonly dv: DataView;
  private readonly data: number;
  readonly length: number;
  private readonly codec: LeReader<T>;

  constructor(buf: Uint8Array, dv: DataView, data: number, length: number, codec: LeReader<T>) {
    this.buf = buf;
    this.dv = dv;
    this.data = data;
    this.length = length;
    this.codec = codec;
  }

  get(index: number): T | undefined {
    if (index < 0 || index >= this.length) return undefined;
    return this.codec.read(this.dv, this.data + index * this.codec.size);
  }

  asBytes(): Uint8Array {
    return this.buf.subarray(this.data, this.data + this.length * this.codec.size);
  }

  *[Symbol.iterator](): Iterator<T> {
    for (let i = 0; i < this.length; i++) {
      yield this.codec.read(this.dv, this.data + i * this.codec.size);
    }
  }
}

/** Low-level view over `[Table]` fixed parts. */
export class TableList implements Iterable<TableRef> {
  private readonly buf: Uint8Array;
  private readonly dv: DataView;
  private readonly first: number;
  readonly length: number;
  readonly elemFixedLen: number;

  constructor(buf: Uint8Array, dv: DataView, first: number, length: number, elemFixedLen: number) {
    this.buf = buf;
    this.dv = dv;
    this.first = first;
    this.length = length;
    this.elemFixedLen = elemFixedLen;
  }

  get(index: number): TableRef {
    if (index < 0 || index >= this.length) {
      throw new GravenError("OutOfBounds");
    }
    return TableRef.inline(this.buf, this.first + index * this.elemFixedLen, this.elemFixedLen, this.dv);
  }

  *[Symbol.iterator](): Iterator<TableRef> {
    for (let i = 0; i < this.length; i++) {
      yield TableRef.inline(this.buf, this.first + i * this.elemFixedLen, this.elemFixedLen, this.dv);
    }
  }
}

/** View over `[string]` or `[bytes]`. Throws `OutOfBounds`, `BadOffset`, or `InvalidUtf8` on element access. */
export class StrList implements Iterable<string> {
  private readonly buf: Uint8Array;
  private readonly dv: DataView;
  private readonly slots: number;
  readonly length: number;

  constructor(buf: Uint8Array, dv: DataView, slots: number, length: number) {
    this.buf = buf;
    this.dv = dv;
    this.slots = slots;
    this.length = length;
  }

  getBytes(index: number): Uint8Array {
    if (index < 0 || index >= this.length) {
      throw new GravenError("OutOfBounds");
    }
    const pos = resolvePtrAt(this.buf, this.dv, this.slots + index * 4);
    if (pos === undefined) return new Uint8Array(0);
    return bytesAt(this.buf, this.dv, pos);
  }

  get(index: number): string {
    return decodeUtf8(this.getBytes(index));
  }

  *[Symbol.iterator](): Iterator<string> {
    for (let i = 0; i < this.length; i++) {
      yield this.get(i);
    }
  }
}

/** View over `[bytes]`, yielding `Uint8Array` slices. */
export class BytesList implements Iterable<Uint8Array> {
  readonly _inner: StrList;

  constructor(inner: StrList) {
    this._inner = inner;
  }

  get length(): number {
    return this._inner.length;
  }

  get(index: number): Uint8Array {
    return this._inner.getBytes(index);
  }

  *[Symbol.iterator](): Iterator<Uint8Array> {
    for (let i = 0; i < this._inner.length; i++) {
      yield this._inner.getBytes(i);
    }
  }
}

/** Pre-verified `[string]` view (skips re-validating UTF-8). */
export class VStrList implements Iterable<string> {
  private readonly inner: StrList;

  constructor(inner: StrList) {
    this.inner = inner;
  }

  get length(): number {
    return this.inner.length;
  }

  getBytes(index: number): Uint8Array {
    return this.inner.getBytes(index);
  }

  get(index: number): string {
    return UTF8_DECODER_LOOSE.decode(this.inner.getBytes(index));
  }

  *[Symbol.iterator](): Iterator<string> {
    for (let i = 0; i < this.inner.length; i++) {
      yield this.get(i);
    }
  }
}

// ---- Verification context ----

export const DEFAULT_MAX_DEPTH = 64;

/** Stateful recursion depth and byte-budget tracker (spec §7). */
export class VerifyCtx {
  readonly maxDepth: number;
  readonly budget: number;
  private depth = 0;
  private remaining: number;

  constructor(bufLen: number, maxDepth = DEFAULT_MAX_DEPTH, maxBudget?: number) {
    this.maxDepth = maxDepth;
    this.budget = maxBudget ?? Math.max(bufLen * 8, 1 << 20);
    this.remaining = this.budget;
  }

  enter(): void {
    if (this.depth >= this.maxDepth) {
      throw new GravenError("DepthExceeded");
    }
    this.depth++;
  }

  leave(): void {
    this.depth--;
  }

  charge(bytes: number): void {
    if (bytes > this.remaining) {
      throw new GravenError("BudgetExceeded");
    }
    this.remaining -= bytes;
  }

  get currentDepth(): number {
    return this.depth;
  }

  get budgetRemaining(): number {
    return this.remaining;
  }
}

// ---- Generated runtime helpers ----

export type Req =
  | { readonly kind: "slot"; readonly off: number; readonly size: number }
  | { readonly kind: "scalar"; readonly name: string; readonly off: number; readonly size: number; readonly bit: Bit }
  | { readonly kind: "bool"; readonly name: string; readonly bit: Bit }
  | { readonly kind: "ptr"; readonly name: string; readonly off: number }
  | { readonly kind: "union"; readonly name: string; readonly off: number };

export interface ReqMask {
  readonly len: number;
  readonly bits: readonly (readonly [number, number])[];
  readonly ptrs: readonly number[];
  readonly unions: readonly number[];
}

export function checkRequired(t: TableRef, reqs: readonly Req[]): void {
  for (const r of reqs) {
    switch (r.kind) {
      case "slot":
        t.slotState(r.off, r.size);
        break;
      case "scalar":
        if (!t.slotState(r.off, r.size) || !t.bit(r.bit)) {
          throw new GravenError("Missing", { field: r.name });
        }
        break;
      case "bool":
        if (!t.bit(r.bit)) {
          throw new GravenError("Missing", { field: r.name });
        }
        break;
      case "ptr":
        if (!t.slotState(r.off, 4) || t.rawI32(r.off) === 0) {
          throw new GravenError("Missing", { field: r.name });
        }
        break;
      case "union":
        if (!t.slotState(r.off, 6) || t.rawU16(r.off) === 0 || t.rawI32(r.off + 2) === 0) {
          throw new GravenError("Missing", { field: r.name });
        }
        break;
    }
  }
}

export function checkRequiredFast(t: TableRef, mask: ReqMask, reqs: readonly Req[]): void {
  if (t.fixedLen >= mask.len) {
    let ok = true;
    for (const [b, m] of mask.bits) {
      if ((t.fixedByte(b) & m) !== m) {
        ok = false;
        break;
      }
    }
    if (ok) {
      for (const o of mask.ptrs) {
        if (t.rawI32(o) === 0) {
          ok = false;
          break;
        }
      }
    }
    if (ok) {
      for (const o of mask.unions) {
        if (t.rawU16(o) === 0 || t.rawI32(o + 2) === 0) {
          ok = false;
          break;
        }
      }
    }
    if (ok) return;
  }
  checkRequired(t, reqs);
}

/** Typed list of verified or checked table readers; an absent list reads as empty. */
export class TableViewList<T> implements Iterable<T> {
  private readonly inner: TableList | undefined;
  private readonly wrap: (t: TableRef) => T;

  constructor(inner: TableList | undefined, wrap: (t: TableRef) => T) {
    this.inner = inner;
    this.wrap = wrap;
  }

  get length(): number {
    return this.inner === undefined ? 0 : this.inner.length;
  }

  get(index: number): T {
    if (this.inner === undefined) {
      throw new GravenError("OutOfBounds");
    }
    return this.wrap(this.inner.get(index));
  }

  *[Symbol.iterator](): Iterator<T> {
    if (this.inner === undefined) return;
    for (let i = 0; i < this.inner.length; i++) {
      yield this.wrap(this.inner.get(i));
    }
  }
}

export function verifyStrs(list: StrList | undefined, ctx: VerifyCtx): void {
  if (list === undefined) return;
  ctx.charge(4 + list.length * 4);
  for (let i = 0; i < list.length; i++) {
    const s = list.getBytes(i);
    decodeUtf8(s);
    ctx.charge(4 + s.length);
  }
}

export function verifyBytesList(list: StrList | undefined, ctx: VerifyCtx): void {
  if (list === undefined) return;
  ctx.charge(4 + list.length * 4);
  for (let i = 0; i < list.length; i++) {
    const b = list.getBytes(i);
    ctx.charge(4 + b.length);
  }
}

export function verifyTableList<T extends { _verifyContent(ctx: VerifyCtx): void }>(
  list: TableList | undefined,
  fromRef: (t: TableRef) => T,
  ctx: VerifyCtx,
): void {
  if (list === undefined) return;
  const per = Math.max(list.elemFixedLen, 1);
  ctx.charge(6 + list.length * per);
  for (let i = 0; i < list.length; i++) {
    fromRef(list.get(i))._verifyContent(ctx);
  }
}

// ---- CRC32C & Envelope (spec §9) ----

const CRC32C_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i++) {
    let crc = i;
    for (let j = 0; j < 8; j++) {
      crc = (crc & 1) !== 0 ? (crc >>> 1) ^ 0x82f63b78 : crc >>> 1;
    }
    table[i] = crc >>> 0;
  }
  return table;
})();

/** Computes CRC-32C (Castagnoli) over `data` as an unsigned 32-bit integer. */
export function crc32c(data: Uint8Array): number {
  let crc = 0xffffffff;
  for (let i = 0; i < data.length; i++) {
    crc = (CRC32C_TABLE[(crc ^ data[i]!) & 0xff]! ^ (crc >>> 8)) >>> 0;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const U64_MASK = 0xffff_ffff_ffff_ffffn;
const UTF8_ENCODER = new TextEncoder();

/** FNV-1a 64-bit hash of `"namespace.Type"` or `"namespace.Service"` (spec §9.2). */
export function schemaId(qualifiedName: string): bigint {
  const bytes = UTF8_ENCODER.encode(qualifiedName);
  let hash = FNV_OFFSET;
  for (let i = 0; i < bytes.length; i++) {
    hash ^= BigInt(bytes[i]!);
    hash = (hash * FNV_PRIME) & U64_MASK;
  }
  return hash;
}

export const ENVELOPE_HEADER_LEN = 20;
export const ENVELOPE_TRAILER_LEN = 4;
const ENVELOPE_VERSION = 1;
const I32_MAX = 0x7fff_ffff;

export interface Envelope {
  readonly schemaId: bigint;
  readonly payload: Uint8Array;
  readonly totalLen: number;
}

/** Writes the 20-byte envelope header at `out[offset..offset+20]`. */
export function writeEnvelopeHeader(
  out: Uint8Array,
  offset: number,
  id: bigint,
  payloadLen: number,
): void {
  out[offset] = 0x47; // 'G'
  out[offset + 1] = 0x52; // 'R'
  out[offset + 2] = 0x56; // 'V'
  out[offset + 3] = 0x4e; // 'N'
  out[offset + 4] = ENVELOPE_VERSION;
  out[offset + 5] = 0;
  out[offset + 6] = 0;
  out[offset + 7] = 0;
  const dv = viewOf(out);
  dv.setBigUint64(offset + 8, id, true);
  dv.setUint32(offset + 16, payloadLen, true);
}

/** Encodes `payload` inside a GRVN v1 envelope with CRC32C trailer. */
export function encodeEnvelope(id: bigint, payload: Uint8Array): Uint8Array {
  if (payload.length > I32_MAX) {
    throw new GravenError("TooLarge");
  }
  const out = new Uint8Array(ENVELOPE_HEADER_LEN + payload.length + ENVELOPE_TRAILER_LEN);
  writeEnvelopeHeader(out, 0, id, payload.length);
  out.set(payload, ENVELOPE_HEADER_LEN);
  const crc = crc32c(out.subarray(0, ENVELOPE_HEADER_LEN + payload.length));
  viewOf(out).setUint32(ENVELOPE_HEADER_LEN + payload.length, crc, true);
  return out;
}

/** Decodes and verifies a GRVN v1 envelope header and CRC32C checksum. */
export function decodeEnvelope(buf: Uint8Array): Envelope {
  if (buf.length < ENVELOPE_HEADER_LEN) {
    throw new GravenError("Incomplete", { needed: ENVELOPE_HEADER_LEN });
  }
  if (buf[0] !== 0x47 || buf[1] !== 0x52 || buf[2] !== 0x56 || buf[3] !== 0x4e) {
    throw new GravenError("BadMagic");
  }
  const version = buf[4]!;
  if (version !== ENVELOPE_VERSION) {
    throw new GravenError("UnsupportedVersion", { found: version });
  }
  const dv = viewOf(buf);
  const flags = buf[5]!;
  const reserved = dv.getUint16(6, true);
  if (flags !== 0 || reserved !== 0) {
    throw new GravenError("BadHeader");
  }
  const id = dv.getBigUint64(8, true);
  const payloadLen = dv.getUint32(16, true);
  if (payloadLen > I32_MAX) {
    throw new GravenError("TooLarge");
  }
  const totalLen = ENVELOPE_HEADER_LEN + payloadLen + ENVELOPE_TRAILER_LEN;
  if (buf.length < totalLen) {
    throw new GravenError("Incomplete", { needed: totalLen });
  }
  const crcPos = ENVELOPE_HEADER_LEN + payloadLen;
  const expectedCrc = dv.getUint32(crcPos, true);
  const actualCrc = crc32c(buf.subarray(0, crcPos));
  if (actualCrc !== expectedCrc) {
    throw new GravenError("ChecksumMismatch", { expected: expectedCrc, found: actualCrc });
  }
  return {
    schemaId: id,
    payload: buf.subarray(ENVELOPE_HEADER_LEN, crcPos),
    totalLen,
  };
}

/** Iterates over concatenated GRVN v1 envelopes in `buf`. */
export function* envelopes(buf: Uint8Array): Generator<Envelope> {
  let offset = 0;
  while (offset < buf.length) {
    const env = decodeEnvelope(buf.subarray(offset));
    offset += env.totalLen;
    yield env;
  }
}

/** Decodes an envelope and validates that its `schemaId` matches `expected`. */
export function openEnvelope(buf: Uint8Array, expected: bigint): Uint8Array {
  const env = decodeEnvelope(buf);
  if (env.schemaId !== expected) {
    throw new GravenError("SchemaMismatch", { expected, found: env.schemaId });
  }
  return env.payload;
}


// ---- Canonical writer (spec §3–§6) ----

class GrowBuf {
  buf: Uint8Array;
  dv: DataView;
  len = 0;

  constructor(initialCapacity = 256) {
    const cap = Math.max(initialCapacity, 64);
    this.buf = new Uint8Array(cap);
    this.dv = new DataView(this.buf.buffer);
  }

  ensure(extra: number): void {
    const needed = this.len + extra;
    if (needed <= this.buf.length) return;
    let nextCap = this.buf.length * 2;
    while (nextCap < needed) nextCap *= 2;
    const next = new Uint8Array(nextCap);
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
    this.dv = new DataView(this.buf.buffer);
  }

  allocZero(size: number): number {
    this.ensure(size);
    const pos = this.len;
    this.buf.fill(0, pos, pos + size);
    this.len += size;
    return pos;
  }

  pushBytes(bytes: Uint8Array): void {
    this.ensure(bytes.length);
    this.buf.set(bytes, this.len);
    this.len += bytes.length;
  }

  pushU32(value: number): void {
    const pos = this.allocZero(4);
    this.dv.setUint32(pos, value, true);
  }

  pushU16(value: number): void {
    const pos = this.allocZero(2);
    this.dv.setUint16(pos, value, true);
  }

  copySlice(start: number, end: number): Uint8Array {
    return this.buf.slice(start, end);
  }

  clear(): void {
    this.len = 0;
  }
}

function allocTableW(main: GrowBuf, fixedLen: number): number {
  const pos = main.allocZero(2 + fixedLen);
  main.dv.setUint16(pos, fixedLen, true);
  return pos + 2;
}

/**
 * Forward-only canonical message builder matching Rust `graven::Builder` byte-for-byte.
 * Start each message with `root(fixedLen)`, populate fields, then emit with
 * `finish()` (bare message), `finishEnvelope(schemaId)`, `seal()`, or `sealEnvelope(schemaId)`.
 */
export class Builder {
  private readonly main: GrowBuf;
  private sealed = 0;
  private msgStart: number | undefined;
  private sealMode: "frame" | "envelope" | undefined;
  private pendingClose: (() => void)[] = [];

  constructor(initialCapacity = 256) {
    this.main = new GrowBuf(initialCapacity);
  }

  root(fixedLen: number): TableW {
    this.pendingClose = [];
    this.main.len = this.sealed;
    const prefix = this.sealMode === "frame" ? 4 : ENVELOPE_HEADER_LEN;
    this.main.allocZero(prefix);
    const ms = this.main.len;
    this.msgStart = ms;
    const start = allocTableW(this.main, fixedLen);
    return new TableW(this.main, undefined, start, this.pendingClose);
  }

  private flushPending(): void {
    while (this.pendingClose.length > 0) {
      this.pendingClose.pop()!();
    }
  }

  /** Returns the current bare message (throws `TooLarge` if > `i32::MAX`). */
  finish(): Uint8Array {
    if (this.sealed !== 0) {
      throw new Error("finish: sealed frames pending; call takeFrames() first");
    }
    this.flushPending();
    const ms = this.msgStart;
    this.msgStart = undefined;
    if (ms === undefined) {
      this.main.clear();
      return new Uint8Array(0);
    }
    const len = this.main.len - ms;
    if (len > I32_MAX) {
      this.main.clear();
      throw new GravenError("TooLarge");
    }
    const out = this.main.copySlice(ms, this.main.len);
    this.main.clear();
    return out;
  }

  /** Wraps the current message in a GRVN v1 envelope and clears the builder. */
  finishEnvelope(id: bigint): Uint8Array {
    if (this.sealed !== 0) {
      throw new Error("finishEnvelope: sealed frames pending; call takeFrames() first");
    }
    this.flushPending();
    const ms = this.msgStart;
    this.msgStart = undefined;
    if (ms === undefined) {
      this.main.clear();
      return new Uint8Array(0);
    }
    const payloadLen = this.main.len - ms;
    if (payloadLen > I32_MAX) {
      this.main.clear();
      throw new GravenError("TooLarge");
    }
    this.closeEnvelope(ms, id, payloadLen);
    const out = this.main.copySlice(ms - ENVELOPE_HEADER_LEN, this.main.len);
    this.main.clear();
    return out;
  }

  /** Seals the current message as `[u32 len][message]` into the pending batch. */
  seal(): void {
    if (this.sealMode === "envelope") {
      throw new Error("builder mode mismatch: already sealing envelopes");
    }
    this.flushPending();
    const ms = this.msgStart;
    this.msgStart = undefined;
    if (ms === undefined) return;
    const len = this.main.len - ms;
    if (len > I32_MAX) {
      this.main.len = this.sealed;
      throw new GravenError("TooLarge");
    }
    // Shift left if initial root reserved ENVELOPE_HEADER_LEN instead of 4.
    const slotStart = this.sealed;
    if (ms - slotStart !== 4) {
      this.main.buf.copyWithin(slotStart + 4, ms, this.main.len);
      this.main.len = slotStart + 4 + len;
    }
    this.sealMode = "frame";
    this.main.dv.setUint32(slotStart, len, true);
    this.sealed = this.main.len;
  }

  /** Seals the current message as a GRVN v1 envelope into the pending batch. */
  sealEnvelope(id: bigint): void {
    if (this.sealMode === "frame") {
      throw new Error("builder mode mismatch: already sealing length-prefixed frames");
    }
    this.flushPending();
    const ms = this.msgStart;
    this.msgStart = undefined;
    if (ms === undefined) return;
    const payloadLen = this.main.len - ms;
    if (payloadLen > I32_MAX) {
      this.main.len = this.sealed;
      throw new GravenError("TooLarge");
    }
    this.sealMode = "envelope";
    this.closeEnvelope(ms, id, payloadLen);
    this.sealed = this.main.len;
  }

  get sealedLen(): number {
    return this.sealed;
  }

  takeFrames(): Uint8Array {
    const out = this.main.copySlice(0, this.sealed);
    this.main.clear();
    this.sealed = 0;
    this.msgStart = undefined;
    this.sealMode = undefined;
    return out;
  }

  private closeEnvelope(ms: number, id: bigint, payloadLen: number): void {
    const hdrStart = ms - ENVELOPE_HEADER_LEN;
    writeEnvelopeHeader(this.main.buf, hdrStart, id, payloadLen);
    const crc = crc32c(this.main.buf.subarray(hdrStart, this.main.len));
    this.main.pushU32(crc);
  }
}

/** Writer for one table's fixed and variable fields. */
export class TableW {
  readonly main: GrowBuf;
  private readonly aux: GrowBuf | undefined;
  private readonly start: number;
  readonly pendingClose: (() => void)[];

  constructor(main: GrowBuf, aux: GrowBuf | undefined, start: number, pendingClose: (() => void)[] = []) {
    this.main = main;
    this.aux = aux;
    this.start = start;
    this.pendingClose = pendingClose;
  }

  private fixedTarget(): GrowBuf {
    return this.aux ?? this.main;
  }

  private setBit(b: Bit): void {
    const t = this.fixedTarget();
    const idx = this.start + b.byte;
    t.buf[idx] = (t.buf[idx] ?? 0) | (1 << b.shift);
  }

  private clearBit(b: Bit): void {
    const t = this.fixedTarget();
    const idx = this.start + b.byte;
    t.buf[idx] = (t.buf[idx] ?? 0) & ~(1 << b.shift);
  }

  patch(off: number, target: number): void {
    const slot = this.start + off;
    const t = this.fixedTarget();
    const val = this.aux !== undefined ? target + 1 : target - slot;
    t.dv.setInt32(slot, val, true);
  }

  set<T>(off: number, presence: Bit, codec: LeCodec<T>, value: T): this {
    const t = this.fixedTarget();
    if (codec.size > 0) {
      codec.write(t.dv, this.start + off, value);
    }
    this.setBit(presence);
    return this;
  }

  setBool(presence: Bit, value: boolean): this {
    this.setBit(presence);
    const valBit: Bit = { byte: presence.byte, shift: presence.shift + 1 };
    if (value) this.setBit(valBit);
    else this.clearBit(valBit);
    return this;
  }

  setBytes(off: number, bytes: Uint8Array): this {
    const pos = this.main.len;
    this.main.pushU32(bytes.length);
    this.main.pushBytes(bytes);
    this.patch(off, pos);
    return this;
  }

  setStr(off: number, s: string): this {
    return this.setBytes(off, UTF8_ENCODER.encode(s));
  }

  setScalars<T>(off: number, codec: LeCodec<T>, values: Iterable<T>): this {
    const items = Array.isArray(values) ? (values as readonly T[]) : [...values];
    const pos = this.main.len;
    this.main.allocZero(4 + items.length * codec.size);
    this.main.dv.setUint32(pos, items.length, true);
    if (codec.size > 0) {
      let cur = pos + 4;
      for (let i = 0; i < items.length; i++) {
        codec.write(this.main.dv, cur, items[i]!);
        cur += codec.size;
      }
    }
    this.patch(off, pos);
    return this;
  }

  setStrs(off: number, values: Iterable<string>): this {
    const items = [...values].map((s) => UTF8_ENCODER.encode(s));
    return this.setBytesList(off, items);
  }

  setBytesList(off: number, values: Iterable<Uint8Array>): this {
    const items = Array.isArray(values) ? (values as readonly Uint8Array[]) : [...values];
    const pos = this.main.len;
    this.main.allocZero(4 + 4 * items.length);
    this.main.dv.setUint32(pos, items.length, true);
    for (let i = 0; i < items.length; i++) {
      const slot = pos + 4 + 4 * i;
      const target = this.main.len;
      const b = items[i]!;
      this.main.pushU32(b.length);
      this.main.pushBytes(b);
      this.main.dv.setInt32(slot, target - slot, true);
    }
    this.patch(off, pos);
    return this;
  }

  table(off: number, fixedLen: number): TableW {
    const pos = this.main.len;
    const start = allocTableW(this.main, fixedLen);
    this.patch(off, pos);
    return new TableW(this.main, undefined, start, this.pendingClose);
  }

  union(off: number, tag: number, fixedLen: number): TableW {
    if (tag === 0) {
      throw new Error("union tag 0 is reserved for absent");
    }
    const t = this.fixedTarget();
    t.dv.setUint16(this.start + off, tag, true);
    return this.table(off + 2, fixedLen);
  }

  tables(off: number, count: number, elemFixedLen: number): TableListW {
    const size = count * elemFixedLen;
    if (size > I32_MAX) {
      throw new GravenError("TooLarge");
    }
    const pos = this.main.len;
    const first = pos + 6;
    this.main.allocZero(6 + size);
    this.main.dv.setUint16(pos + 4, elemFixedLen, true);
    this.patch(off, pos);
    return new TableListW(this.main, pos, first, elemFixedLen, count, this.pendingClose);
  }

  tablesStreaming(off: number, elemFixedLen: number, ptrSlots: readonly number[]): StreamListW {
    const list = new StreamListW(this, off, elemFixedLen, ptrSlots);
    this.pendingClose.push(() => list.close());
    return list;
  }

  scalarsStreaming<T>(off: number, codec: LeCodec<T>): ScalarListW<T> {
    const pos = this.main.len;
    this.main.allocZero(4);
    this.patch(off, pos);
    return new ScalarListW(this.main, pos, codec);
  }
}

/** Fixed-count list-of-tables writer; updates count on every `push()`. */
export class TableListW {
  private readonly main: GrowBuf;
  private readonly pos: number;
  private readonly first: number;
  private readonly efl: number;
  private readonly cap: number;
  private readonly pendingClose: (() => void)[];
  private count = 0;

  constructor(
    main: GrowBuf,
    pos: number,
    first: number,
    efl: number,
    cap: number,
    pendingClose: (() => void)[] = [],
  ) {
    this.main = main;
    this.pos = pos;
    this.first = first;
    this.efl = efl;
    this.cap = cap;
    this.pendingClose = pendingClose;
  }

  push(): TableW {
    if (this.count >= this.cap) {
      throw new Error("TableListW: pushed more elements than reserved");
    }
    const start = this.first + this.count * this.efl;
    this.count++;
    this.main.dv.setUint32(this.pos, this.count, true);
    return new TableW(this.main, undefined, start, this.pendingClose);
  }

  close(): void {
    this.main.dv.setUint32(this.pos, this.count, true);
  }
}

/** Streaming list-of-tables writer; finalizes on `close()`. */
export class StreamListW {
  private readonly parent: TableW;
  private readonly off: number;
  private readonly efl: number;
  private readonly ptrSlots: readonly number[];
  private readonly aux = new GrowBuf(128);
  private count = 0;
  private closed = false;

  constructor(parent: TableW, off: number, efl: number, ptrSlots: readonly number[]) {
    this.parent = parent;
    this.off = off;
    this.efl = efl;
    this.ptrSlots = ptrSlots;
  }

  push(): TableW {
    if (this.closed) {
      throw new Error("StreamListW: already closed");
    }
    const start = this.aux.allocZero(this.efl);
    this.count++;
    return new TableW(this.parent.main, this.aux, start, this.parent.pendingClose);
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    const main = this.parent.main;
    const header = main.len;
    main.pushU32(this.count);
    main.pushU16(this.efl);
    const base = main.len;
    main.pushBytes(this.aux.buf.subarray(0, this.aux.len));
    for (let i = 0; i < this.count; i++) {
      for (const slot of this.ptrSlots) {
        const p = base + i * this.efl + slot;
        const stored = main.dv.getInt32(p, true);
        if (stored !== 0) {
          main.dv.setInt32(p, stored - 1 - p, true);
        }
      }
    }
    this.parent.patch(this.off, header);
  }
}

/** Streaming scalar list writer; updates count on every `push()`. */
export class ScalarListW<T> {
  private readonly main: GrowBuf;
  private readonly pos: number;
  private readonly codec: LeCodec<T>;
  private count = 0;

  constructor(main: GrowBuf, pos: number, codec: LeCodec<T>) {
    this.main = main;
    this.pos = pos;
    this.codec = codec;
  }

  push(value: T): this {
    if (this.codec.size > 0) {
      const off = this.main.allocZero(this.codec.size);
      this.codec.write(this.main.dv, off, value);
    }
    this.count++;
    this.main.dv.setUint32(this.pos, this.count, true);
    return this;
  }

  close(): void {
    this.main.dv.setUint32(this.pos, this.count, true);
  }
}


// ---- Typed generated-code wrappers & validation ----

export function utf8Len(s: string): number {
  return UTF8_ENCODER.encode(s).length;
}

export function reqPtr<T>(val: T | undefined, field: string): T {
  if (val === undefined) {
    throw new GravenError("Missing", { field });
  }
  return val;
}

export function optVStrs(s: StrList | undefined): VStrList | undefined {
  return s === undefined ? undefined : new VStrList(s);
}

export function reqVStrs(s: StrList | undefined, field: string): VStrList {
  return new VStrList(reqPtr(s, field));
}

/** Typed fixed-count list writer returning typed element writers `W`. */
export class ListW<W> {
  private readonly inner: TableListW;
  private readonly wrap: (w: TableW) => W;

  constructor(inner: TableListW, wrap: (w: TableW) => W) {
    this.inner = inner;
    this.wrap = wrap;
  }

  push(): W {
    return this.wrap(this.inner.push());
  }

  close(): void {
    this.inner.close();
  }
}

/** Typed streaming list writer returning typed element writers `W`. */
export class StreamW<W> {
  private readonly inner: StreamListW;
  private readonly wrap: (w: TableW) => W;

  constructor(inner: StreamListW, wrap: (w: TableW) => W) {
    this.inner = inner;
    this.wrap = wrap;
  }

  push(): W {
    return this.wrap(this.inner.push());
  }

  close(): void {
    this.inner.close();
  }
}

/**
 * A value that breaks a rule of the schema. It carries codes, never text: the application
 * turns them into a message in the reader's language.
 */
export interface ValidationFailure {
  readonly table: string;
  readonly field?: string;
  readonly index?: number;
  /** `"range"`, `"len"`, `"finite"`, `"known"` or `"check(name)"`. */
  readonly rule: string;
  /** `range`/`len`: the declared lower bound, as a decimal number. */
  readonly min?: string;
  /** `range`/`len`: the declared upper bound, as a decimal number. */
  readonly max?: string;
  /** `check(name)`: the code the check returned. */
  readonly reason?: string;
}

/** Adds the code a check returned (`r`) to a failure; used by generated code. */
export function checkFailure(f: ValidationFailure, r: unknown): ValidationFailure {
  return typeof r === "string" ? { ...f, reason: r } : f;
}

/** Thrown by `validate()` when a schema validation rule fails. */
export class ValidationError extends Error {
  readonly failure: ValidationFailure;

  constructor(failure: ValidationFailure) {
    const loc =
      failure.field !== undefined
        ? failure.index !== undefined
          ? `${failure.table}.${failure.field}[${failure.index}]`
          : `${failure.table}.${failure.field}`
        : failure.table;
    const extra = [
      failure.min !== undefined ? `min ${failure.min}` : "",
      failure.max !== undefined ? `max ${failure.max}` : "",
      failure.reason !== undefined ? failure.reason : "",
    ].filter((s) => s !== "");
    super(`validation failed on ${loc}: ${failure.rule}${extra.length > 0 ? ` (${extra.join(", ")})` : ""}`);
    this.name = "ValidationError";
    this.failure = failure;
  }
}

// ---- RPC v1 frames, Fault, and client transport helpers ----

export const FRAME_VERSION = 1;
export const FRAME_HEADER_LEN = 16;

export const FrameKind = {
  Call: 1,
  Ok: 2,
  Err: 3,
  Item: 4,
  End: 5,
  Fault: 6,
} as const;
export type FrameKind = (typeof FrameKind)[keyof typeof FrameKind];

export const FaultCode = {
  Unsupported: 1,
  Malformed: 2,
  Invalid: 3,
  DeadlineExceeded: 4,
  Internal: 5,
  Unavailable: 6,
  isKnown(code: number): boolean {
    return code >= 1 && code <= 6;
  },
  normalize(code: number): number {
    return code >= 1 && code <= 6 ? code : 5;
  },
} as const;

export const BROKEN_RULE_SCHEMA_ID = schemaId("graven.rpc.BrokenRule");
export const FAULT_SCHEMA_ID = schemaId("graven.rpc.Fault");

const FAULT_FIXED_LEN = 11;
/** `BrokenRule` up to `rule` (@3): written when `min`, `max` and `reason` are absent. */
const BROKEN_RULE_FIXED_LEN_V0 = 17;
/** `BrokenRule` with `min`, `max` and `reason` (@4–@6). */
const BROKEN_RULE_FIXED_LEN = 29;

export interface FrameHeader {
  readonly kind: FrameKind;
  readonly method: number;
  readonly variant: number;
  readonly serviceId: bigint;
}

export interface Frame {
  readonly header: FrameHeader;
  /** Envelope `schemaId`, or `0n` for `FrameKind.End`. */
  readonly schemaId: bigint;
  /** Raw Graven message payload inside the envelope, or empty for `FrameKind.End`. */
  readonly payload: Uint8Array;
  /** Total frame byte length (`16` for `End`, or `16 + 24 + payload.length`). */
  readonly totalLen: number;
}

export interface BrokenRule {
  readonly tableName: string;
  readonly field?: string;
  readonly index?: number;
  /** `"range"`, `"len"`, `"finite"`, `"known"` or `"check(name)"`. */
  readonly rule: string;
  /** `range`/`len`: the declared lower bound, as a decimal number. */
  readonly min?: string;
  /** `range`/`len`: the declared upper bound, as a decimal number. */
  readonly max?: string;
  /** `check(name)`: the code the check returned. */
  readonly reason?: string;
}

export interface FaultData {
  readonly code: number;
  readonly message?: string;
  readonly broken?: BrokenRule;
}

function writeFrameHeader(out: Uint8Array, header: FrameHeader): void {
  const dv = new DataView(out.buffer, out.byteOffset, FRAME_HEADER_LEN);
  out[0] = FRAME_VERSION;
  out[1] = header.kind;
  dv.setUint16(2, header.method, true);
  dv.setUint16(4, header.variant, true);
  dv.setUint16(6, 0, true);
  dv.setBigUint64(8, header.serviceId, true);
}

/** Encodes a single RPC v1 frame. */
export function encodeFrame(header: FrameHeader, schemaIdVal: bigint, payload: Uint8Array): Uint8Array {
  if (header.kind === FrameKind.End) {
    if (header.variant !== 0 || payload.length !== 0) {
      throw new GravenError("BadHeader");
    }
    const out = new Uint8Array(FRAME_HEADER_LEN);
    writeFrameHeader(out, header);
    return out;
  }
  const env = encodeEnvelope(schemaIdVal, payload);
  const out = new Uint8Array(FRAME_HEADER_LEN + env.length);
  writeFrameHeader(out, header);
  out.set(env, FRAME_HEADER_LEN);
  return out;
}

/** Encodes a 16-byte `End` RPC frame. */
export function encodeEnd(serviceIdVal: bigint, method: number): Uint8Array {
  return encodeFrame(
    { kind: FrameKind.End, method, variant: 0, serviceId: serviceIdVal },
    0n,
    EMPTY_BYTES,
  );
}

/**
 * Decodes one RPC v1 frame at the start of `buf`, following the normative check order in
 * `docs/spec/graven-rpc-v1.md`:
 * 1. `buf.length < 16` -> `Incomplete { needed: 16 }`
 * 2. `buf[0] !== 1` -> `UnsupportedFrameVersion`
 * 3. `buf[1]` not in `1..=6` -> `BadKind`
 * 4. `reserved !== 0 || (kind === End && variant !== 0)` -> `BadFrameHeader`
 * 5. For `End`, returns `totalLen = 16`. Otherwise decodes `&buf[16..]` as an envelope.
 */
export function decodeFrame(buf: Uint8Array): Frame {
  if (buf.length < FRAME_HEADER_LEN) {
    throw new GravenError("Incomplete", { needed: FRAME_HEADER_LEN });
  }
  const version = buf[0]!;
  if (version !== FRAME_VERSION) {
    throw new GravenError("UnsupportedFrameVersion", { found: version });
  }
  const kindByte = buf[1]!;
  if (kindByte < 1 || kindByte > 6) {
    throw new GravenError("BadKind", { found: kindByte });
  }
  const kind = kindByte as FrameKind;
  const dv = new DataView(buf.buffer, buf.byteOffset, FRAME_HEADER_LEN);
  const method = dv.getUint16(2, true);
  const variant = dv.getUint16(4, true);
  const reserved = dv.getUint16(6, true);
  const serviceIdVal = dv.getBigUint64(8, true);

  if (reserved !== 0 || (kind === FrameKind.End && variant !== 0)) {
    throw new GravenError("BadFrameHeader");
  }

  const header: FrameHeader = { kind, method, variant, serviceId: serviceIdVal };
  if (kind === FrameKind.End) {
    return { header, schemaId: 0n, payload: EMPTY_BYTES, totalLen: FRAME_HEADER_LEN };
  }

  let env: Envelope;
  try {
    env = decodeEnvelope(buf.subarray(FRAME_HEADER_LEN));
  } catch (e) {
    if (e instanceof GravenError && e.kind === "Incomplete") {
      throw new GravenError("Incomplete", { needed: FRAME_HEADER_LEN + (e.needed ?? 0) });
    }
    throw e;
  }

  return {
    header,
    schemaId: env.schemaId,
    payload: env.payload,
    totalLen: FRAME_HEADER_LEN + env.totalLen,
  };
}

/** Iterates over concatenated RPC frames in `buf`. */
export function* frames(buf: Uint8Array): IterableIterator<Frame> {
  let off = 0;
  while (off < buf.length) {
    const f = decodeFrame(buf.subarray(off));
    yield f;
    off += f.totalLen;
  }
}

/** Verifies and decodes a `graven.rpc.Fault` message payload. */
export function decodeFault(payload: Uint8Array): FaultData {
  const ctx = new VerifyCtx(payload.length);
  ctx.enter();
  const t = TableRef.at(payload, 0);
  ctx.charge(2 + t.fixedLen);

  const rawCode = t.scalar(1, { byte: 0, shift: 0 }, leU16);
  if (rawCode === undefined) {
    throw new GravenError("Missing", { field: "code" });
  }
  const message = t.str(3);
  if (message !== undefined) {
    ctx.charge(4 + utf8Len(message));
  }
  let broken: BrokenRule | undefined;
  const b = t.table(7);
  if (b !== undefined) {
    ctx.enter();
    ctx.charge(2 + b.fixedLen);
    const tableName = b.str(0);
    if (tableName === undefined) {
      throw new GravenError("Missing", { field: "table_name" });
    }
    ctx.charge(4 + utf8Len(tableName));
    const field = b.str(4);
    if (field !== undefined) {
      ctx.charge(4 + utf8Len(field));
    }
    const index = b.scalar(9, { byte: 8, shift: 0 }, leU32);
    const rule = b.str(13);
    if (rule === undefined) {
      throw new GravenError("Missing", { field: "rule" });
    }
    ctx.charge(4 + utf8Len(rule));
    const opt = (off: number): string | undefined => {
      const s = b.str(off);
      if (s !== undefined) ctx.charge(4 + utf8Len(s));
      return s;
    };
    const min = opt(17);
    const max = opt(21);
    const reason = opt(25);
    ctx.leave();
    broken = {
      tableName,
      ...(field !== undefined ? { field } : {}),
      ...(index !== undefined ? { index } : {}),
      rule,
      ...(min !== undefined ? { min } : {}),
      ...(max !== undefined ? { max } : {}),
      ...(reason !== undefined ? { reason } : {}),
    };
  }
  ctx.leave();
  return {
    code: rawCode,
    ...(message !== undefined ? { message } : {}),
    ...(broken !== undefined ? { broken } : {}),
  };
}

/** Encodes a raw `graven.rpc.Fault` message payload (no envelope or frame header). */
export function encodeFaultPayload(fault: FaultData): Uint8Array {
  const b = new Builder();
  const w = b.root(FAULT_FIXED_LEN);
  w.set(1, { byte: 0, shift: 0 }, leU16, fault.code);
  if (fault.message !== undefined) {
    w.setStr(3, fault.message);
  }
  if (fault.broken !== undefined) {
    const br = fault.broken;
    const extended = br.min !== undefined || br.max !== undefined || br.reason !== undefined;
    const bw = w.table(7, extended ? BROKEN_RULE_FIXED_LEN : BROKEN_RULE_FIXED_LEN_V0);
    bw.setStr(0, fault.broken.tableName);
    if (fault.broken.field !== undefined) {
      bw.setStr(4, fault.broken.field);
    }
    if (fault.broken.index !== undefined) {
      bw.set(9, { byte: 8, shift: 0 }, leU32, fault.broken.index);
    }
    bw.setStr(13, fault.broken.rule);
    if (br.min !== undefined) bw.setStr(17, br.min);
    if (br.max !== undefined) bw.setStr(21, br.max);
    if (br.reason !== undefined) bw.setStr(25, br.reason);
  }
  return b.finish();
}

/** Encodes a full `Fault` RPC frame. */
export function encodeFaultFrame(serviceIdVal: bigint, method: number, fault: FaultData): Uint8Array {
  return encodeFrame(
    { kind: FrameKind.Fault, method, variant: 0, serviceId: serviceIdVal },
    FAULT_SCHEMA_ID,
    encodeFaultPayload(fault),
  );
}

/** Error thrown when the server replies with a `Fault` frame. */
export class RpcFault extends Error {
  readonly code: number;
  readonly faultMessage?: string;
  readonly broken?: BrokenRule;

  constructor(fault: FaultData) {
    super(fault.message ?? `rpc fault code=${fault.code}`);
    this.name = "RpcFault";
    this.code = fault.code;
    if (fault.message !== undefined) this.faultMessage = fault.message;
    if (fault.broken !== undefined) this.broken = fault.broken;
  }
}

export type RpcProtocolErrorKind =
  | "Frame"
  | "Payload"
  | "EmptyReply"
  | "UnexpectedKind"
  | "ServiceMismatch"
  | "MethodMismatch"
  | "SchemaMismatch"
  | "MissingVariant"
  | "UnexpectedVariant"
  | "TrailingBytes"
  | "UnexpectedOneWayReply"
  | "Truncated";

export interface RpcProtocolErrorDetails {
  readonly causeError?: GravenError;
  readonly frameKind?: number;
  readonly expected?: bigint | number;
  readonly found?: bigint | number;
  readonly variant?: number;
  readonly trailing?: number;
}

/** Error thrown when a reply violates the Graven RPC v1 protocol. */
export class RpcProtocolError extends Error {
  readonly kind: RpcProtocolErrorKind;
  readonly causeError?: GravenError;
  readonly frameKind?: number;
  readonly expected?: bigint | number;
  readonly found?: bigint | number;
  readonly variant?: number;
  readonly trailing?: number;

  constructor(kind: RpcProtocolErrorKind, details?: RpcProtocolErrorDetails) {
    super(formatRpcProtocolError(kind, details));
    this.name = "RpcProtocolError";
    this.kind = kind;
    if (details?.causeError !== undefined) this.causeError = details.causeError;
    if (details?.frameKind !== undefined) this.frameKind = details.frameKind;
    if (details?.expected !== undefined) this.expected = details.expected;
    if (details?.found !== undefined) this.found = details.found;
    if (details?.variant !== undefined) this.variant = details.variant;
    if (details?.trailing !== undefined) this.trailing = details.trailing;
  }
}

function formatRpcProtocolError(kind: RpcProtocolErrorKind, d?: RpcProtocolErrorDetails): string {
  switch (kind) {
    case "Frame":
      return `frame decode error: ${d?.causeError?.message ?? "unknown"}`;
    case "Payload":
      return `payload decode error: ${d?.causeError?.message ?? "unknown"}`;
    case "EmptyReply":
      return "empty reply where a frame was expected";
    case "UnexpectedKind":
      return `unexpected frame kind ${String(d?.frameKind)}`;
    case "ServiceMismatch":
      return `reply service_id mismatch: expected ${String(d?.expected)}, found ${String(d?.found)}`;
    case "MethodMismatch":
      return `reply method mismatch: expected @${String(d?.expected)}, found @${String(d?.found)}`;
    case "SchemaMismatch":
      return `payload schema mismatch: expected ${String(d?.expected)}, found ${String(d?.found)}`;
    case "MissingVariant":
      return "union reply frame is missing variant tag";
    case "UnexpectedVariant":
      return `table reply frame had non-zero variant tag ${String(d?.variant)}`;
    case "TrailingBytes":
      return `trailing bytes (${String(d?.trailing)}) after terminal frame`;
    case "UnexpectedOneWayReply":
      return "one-way call received non-empty reply";
    case "Truncated":
      return "reply stream ended before End or Fault frame";
  }
}

export interface CallOptions {
  timeoutMs?: number;
}

export type ReplyStreamInput =
  | Uint8Array
  | ReadableStream<Uint8Array>
  | AsyncIterable<Uint8Array>;

export type Sender = (
  call: Uint8Array,
  timeoutMs: number | undefined,
) => Promise<ReplyStreamInput>;

export type RpcResult<T, E> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly error: E };

export interface RpcCodec<W, V> {
  encode(fn: (w: W) => void): { readonly variant: number; readonly schemaId: bigint; readonly payload: Uint8Array };
  verify(variant: number, schemaIdVal: bigint, payload: Uint8Array): V;
}

export function rpcVerifyTable<V>(
  variant: number,
  foundSchemaId: bigint,
  payload: Uint8Array,
  expectedSchemaId: bigint,
  verifyFn: (buf: Uint8Array) => V,
): V {
  if (variant !== 0) {
    throw new RpcProtocolError("UnexpectedVariant", { variant });
  }
  if (foundSchemaId !== expectedSchemaId) {
    throw new RpcProtocolError("SchemaMismatch", { expected: expectedSchemaId, found: foundSchemaId });
  }
  try {
    return verifyFn(payload);
  } catch (e) {
    if (e instanceof GravenError) {
      throw new RpcProtocolError("Payload", { causeError: e });
    }
    throw e;
  }
}

export function encodeCallFrame<W, V>(
  codec: RpcCodec<W, V>,
  serviceIdVal: bigint,
  method: number,
  fn: (w: W) => void,
): Uint8Array {
  const { variant, schemaId: sid, payload } = codec.encode(fn);
  return encodeFrame(
    { kind: FrameKind.Call, method, variant, serviceId: serviceIdVal },
    sid,
    payload,
  );
}

function isReadableStream(v: ReplyStreamInput): v is ReadableStream<Uint8Array> {
  return typeof (v as ReadableStream<Uint8Array>).getReader === "function";
}

class ChunkSource {
  private readonly streamReader?: ReadableStreamDefaultReader<Uint8Array>;
  private readonly iter?: AsyncIterator<Uint8Array>;
  private initial?: Uint8Array;
  private done = false;

  constructor(input: ReplyStreamInput) {
    if (input instanceof Uint8Array) {
      this.initial = input;
      if (input.length === 0) this.done = true;
    } else if (isReadableStream(input)) {
      this.streamReader = input.getReader();
    } else {
      this.iter = input[Symbol.asyncIterator]();
    }
  }

  async next(): Promise<Uint8Array | undefined> {
    if (this.initial !== undefined) {
      const c = this.initial;
      this.initial = undefined;
      this.done = true;
      return c.length > 0 ? c : undefined;
    }
    if (this.done) return undefined;
    if (this.streamReader !== undefined) {
      const res = await this.streamReader.read();
      if (res.done) {
        this.done = true;
        return undefined;
      }
      return res.value;
    }
    if (this.iter !== undefined) {
      const res = await this.iter.next();
      if (res.done) {
        this.done = true;
        return undefined;
      }
      return res.value;
    }
    this.done = true;
    return undefined;
  }

  async close(): Promise<void> {
    if (this.streamReader !== undefined) {
      try {
        if (!this.done) await this.streamReader.cancel();
      } catch {
        // ignore cancel errors during cleanup
      } finally {
        this.streamReader.releaseLock();
      }
    } else if (this.iter !== undefined && !this.done && typeof this.iter.return === "function") {
      try {
        await this.iter.return();
      } catch {
        // ignore return errors during cleanup
      }
    }
    this.done = true;
  }
}

async function collectReplyBytes(input: ReplyStreamInput): Promise<Uint8Array> {
  if (input instanceof Uint8Array) return input;
  const src = new ChunkSource(input);
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    while (true) {
      const c = await src.next();
      if (c === undefined) break;
      if (c.length > 0) {
        chunks.push(c);
        total += c.length;
      }
    }
  } finally {
    await src.close();
  }
  if (chunks.length === 0) return EMPTY_BYTES;
  if (chunks.length === 1) return chunks[0]!;
  const out = new Uint8Array(total);
  let pos = 0;
  for (const c of chunks) {
    out.set(c, pos);
    pos += c.length;
  }
  return out;
}

class FrameStreamReader {
  private readonly src: ChunkSource;
  private buf: Uint8Array = new Uint8Array(0);
  private pos = 0;
  private len = 0;
  private eof = false;

  constructor(input: ReplyStreamInput) {
    this.src = new ChunkSource(input);
  }

  trailingCount(): number {
    return this.len - this.pos;
  }

  private append(chunk: Uint8Array): void {
    const avail = this.len - this.pos;
    if (this.len + chunk.length <= this.buf.length) {
      this.buf.set(chunk, this.len);
      this.len += chunk.length;
      return;
    }
    if (avail + chunk.length <= this.buf.length && this.pos > 0) {
      this.buf.copyWithin(0, this.pos, this.len);
      this.pos = 0;
      this.buf.set(chunk, avail);
      this.len = avail + chunk.length;
      return;
    }
    let nextCap = Math.max(256, this.buf.length * 2);
    while (nextCap < avail + chunk.length) nextCap *= 2;
    const grown = new Uint8Array(nextCap);
    if (avail > 0) grown.set(this.buf.subarray(this.pos, this.len), 0);
    grown.set(chunk, avail);
    this.buf = grown;
    this.pos = 0;
    this.len = avail + chunk.length;
  }

  async nextFrame(): Promise<Frame | undefined> {
    while (true) {
      const avail = this.len - this.pos;
      if (avail > 0) {
        const view = this.buf.subarray(this.pos, this.len);
        try {
          const frame = decodeFrame(view);
          // Copy slice out of mutable scratch buffer only when payload aliases or slice is reused.
          const copy = view.slice(0, frame.totalLen);
          this.pos += frame.totalLen;
          if (this.pos === this.len) {
            this.pos = 0;
            this.len = 0;
          }
          return decodeFrame(copy);
        } catch (e) {
          if (e instanceof GravenError && e.kind === "Incomplete") {
            if (this.eof) {
              throw new RpcProtocolError("Truncated");
            }
          } else if (e instanceof GravenError) {
            throw new RpcProtocolError("Frame", { causeError: e });
          } else {
            throw e;
          }
        }
      } else if (this.eof) {
        return undefined;
      }

      const chunk = await this.src.next();
      if (chunk === undefined) {
        this.eof = true;
      } else if (chunk.length > 0) {
        this.append(chunk);
      }
    }
  }

  async close(): Promise<void> {
    await this.src.close();
  }
}

function checkFrameCall(frame: Frame, serviceIdVal: bigint, method: number): void {
  if (frame.header.serviceId !== serviceIdVal) {
    throw new RpcProtocolError("ServiceMismatch", {
      expected: serviceIdVal,
      found: frame.header.serviceId,
    });
  }
  if (frame.header.method !== method) {
    throw new RpcProtocolError("MethodMismatch", {
      expected: method,
      found: frame.header.method,
    });
  }
}

function decodeFaultFrameError(frame: Frame): RpcFault | RpcProtocolError {
  if (frame.header.variant !== 0) {
    return new RpcProtocolError("UnexpectedVariant", { variant: frame.header.variant });
  }
  if (frame.schemaId !== FAULT_SCHEMA_ID) {
    return new RpcProtocolError("SchemaMismatch", {
      expected: FAULT_SCHEMA_ID,
      found: frame.schemaId,
    });
  }
  try {
    return new RpcFault(decodeFault(frame.payload));
  } catch (e) {
    if (e instanceof GravenError) {
      return new RpcProtocolError("Payload", { causeError: e });
    }
    throw e;
  }
}

function decodeTerminalFrame(bytes: Uint8Array, serviceIdVal: bigint, method: number): Frame {
  if (bytes.length === 0) {
    throw new RpcProtocolError("EmptyReply");
  }
  let frame: Frame;
  try {
    frame = decodeFrame(bytes);
  } catch (e) {
    if (e instanceof GravenError) {
      throw new RpcProtocolError("Frame", { causeError: e });
    }
    throw e;
  }
  if (frame.totalLen !== bytes.length) {
    throw new RpcProtocolError("TrailingBytes", { trailing: bytes.length - frame.totalLen });
  }
  checkFrameCall(frame, serviceIdVal, method);
  return frame;
}

export async function rpcOneWay<IW, IV>(
  send: Sender,
  inCodec: RpcCodec<IW, IV>,
  serviceIdVal: bigint,
  method: number,
  fn: (w: IW) => void,
  opts?: CallOptions,
): Promise<void> {
  const call = encodeCallFrame(inCodec, serviceIdVal, method, fn);
  const reply = await send(call, opts?.timeoutMs);
  const bytes = await collectReplyBytes(reply);
  if (bytes.length !== 0) {
    throw new RpcProtocolError("UnexpectedOneWayReply");
  }
}

export async function rpcUnary<IW, IV, OW, OV>(
  send: Sender,
  inCodec: RpcCodec<IW, IV>,
  outCodec: RpcCodec<OW, OV>,
  serviceIdVal: bigint,
  method: number,
  defaultTimeoutMs: number | undefined,
  fn: (w: IW) => void,
  opts?: CallOptions,
): Promise<OV> {
  const call = encodeCallFrame(inCodec, serviceIdVal, method, fn);
  const timeout = opts?.timeoutMs !== undefined ? opts.timeoutMs : defaultTimeoutMs;
  const reply = await send(call, timeout);
  const bytes = await collectReplyBytes(reply);
  const frame = decodeTerminalFrame(bytes, serviceIdVal, method);
  switch (frame.header.kind) {
    case FrameKind.Ok:
      return outCodec.verify(frame.header.variant, frame.schemaId, frame.payload);
    case FrameKind.Fault:
      throw decodeFaultFrameError(frame);
    default:
      throw new RpcProtocolError("UnexpectedKind", { frameKind: frame.header.kind });
  }
}

export async function rpcUnaryResult<IW, IV, OW, OV, EW, EV>(
  send: Sender,
  inCodec: RpcCodec<IW, IV>,
  outCodec: RpcCodec<OW, OV>,
  errCodec: RpcCodec<EW, EV>,
  serviceIdVal: bigint,
  method: number,
  defaultTimeoutMs: number | undefined,
  fn: (w: IW) => void,
  opts?: CallOptions,
): Promise<RpcResult<OV, EV>> {
  const call = encodeCallFrame(inCodec, serviceIdVal, method, fn);
  const timeout = opts?.timeoutMs !== undefined ? opts.timeoutMs : defaultTimeoutMs;
  const reply = await send(call, timeout);
  const bytes = await collectReplyBytes(reply);
  const frame = decodeTerminalFrame(bytes, serviceIdVal, method);
  switch (frame.header.kind) {
    case FrameKind.Ok:
      return { ok: true, value: outCodec.verify(frame.header.variant, frame.schemaId, frame.payload) };
    case FrameKind.Err:
      return { ok: false, error: errCodec.verify(frame.header.variant, frame.schemaId, frame.payload) };
    case FrameKind.Fault:
      throw decodeFaultFrameError(frame);
    default:
      throw new RpcProtocolError("UnexpectedKind", { frameKind: frame.header.kind });
  }
}

async function* iterateStreamFrames<OW, OV>(
  reader: FrameStreamReader,
  first: Frame,
  outCodec: RpcCodec<OW, OV>,
  serviceIdVal: bigint,
  method: number,
): AsyncIterableIterator<OV> {
  let pending: Frame | undefined = first;
  try {
    while (true) {
      const frame = pending ?? (await reader.nextFrame());
      pending = undefined;
      if (frame === undefined) {
        throw new RpcProtocolError("Truncated");
      }
      checkFrameCall(frame, serviceIdVal, method);
      if (frame.header.kind === FrameKind.Item) {
        yield outCodec.verify(frame.header.variant, frame.schemaId, frame.payload);
      } else if (frame.header.kind === FrameKind.End) {
        const trailing = reader.trailingCount();
        if (trailing > 0) {
          throw new RpcProtocolError("TrailingBytes", { trailing });
        }
        return;
      } else if (frame.header.kind === FrameKind.Fault) {
        const trailing = reader.trailingCount();
        if (trailing > 0) {
          throw new RpcProtocolError("TrailingBytes", { trailing });
        }
        throw decodeFaultFrameError(frame);
      } else {
        throw new RpcProtocolError("UnexpectedKind", { frameKind: frame.header.kind });
      }
    }
  } finally {
    await reader.close();
  }
}

async function openStreamReply(
  input: ReplyStreamInput,
  serviceIdVal: bigint,
  method: number,
): Promise<{ reader: FrameStreamReader; first: Frame; err: boolean }> {
  const reader = new FrameStreamReader(input);
  try {
    const first = await reader.nextFrame();
    if (first === undefined) {
      throw new RpcProtocolError("EmptyReply");
    }
    checkFrameCall(first, serviceIdVal, method);
    if (first.header.kind === FrameKind.Item || first.header.kind === FrameKind.End) {
      return { reader, first, err: false };
    }
    if (first.header.kind === FrameKind.Err) {
      const trailing = reader.trailingCount();
      if (trailing > 0) {
        throw new RpcProtocolError("TrailingBytes", { trailing });
      }
      return { reader, first, err: true };
    }
    if (first.header.kind === FrameKind.Fault) {
      const trailing = reader.trailingCount();
      if (trailing > 0) {
        throw new RpcProtocolError("TrailingBytes", { trailing });
      }
      throw decodeFaultFrameError(first);
    }
    throw new RpcProtocolError("UnexpectedKind", { frameKind: first.header.kind });
  } catch (e) {
    await reader.close();
    throw e;
  }
}

export async function rpcStream<IW, IV, OW, OV>(
  send: Sender,
  inCodec: RpcCodec<IW, IV>,
  outCodec: RpcCodec<OW, OV>,
  serviceIdVal: bigint,
  method: number,
  defaultTimeoutMs: number | undefined,
  fn: (w: IW) => void,
  opts?: CallOptions,
): Promise<AsyncIterable<OV>> {
  const call = encodeCallFrame(inCodec, serviceIdVal, method, fn);
  const timeout = opts?.timeoutMs !== undefined ? opts.timeoutMs : defaultTimeoutMs;
  const reply = await send(call, timeout);
  const { reader, first, err } = await openStreamReply(reply, serviceIdVal, method);
  if (err) {
    await reader.close();
    throw new RpcProtocolError("UnexpectedKind", { frameKind: FrameKind.Err });
  }
  return iterateStreamFrames(reader, first, outCodec, serviceIdVal, method);
}

export async function rpcStreamResult<IW, IV, OW, OV, EW, EV>(
  send: Sender,
  inCodec: RpcCodec<IW, IV>,
  outCodec: RpcCodec<OW, OV>,
  errCodec: RpcCodec<EW, EV>,
  serviceIdVal: bigint,
  method: number,
  defaultTimeoutMs: number | undefined,
  fn: (w: IW) => void,
  opts?: CallOptions,
): Promise<RpcResult<AsyncIterable<OV>, EV>> {
  const call = encodeCallFrame(inCodec, serviceIdVal, method, fn);
  const timeout = opts?.timeoutMs !== undefined ? opts.timeoutMs : defaultTimeoutMs;
  const reply = await send(call, timeout);
  const { reader, first, err } = await openStreamReply(reply, serviceIdVal, method);
  if (err) {
    await reader.close();
    return {
      ok: false,
      error: errCodec.verify(first.header.variant, first.schemaId, first.payload),
    };
  }
  return {
    ok: true,
    value: iterateStreamFrames(reader, first, outCodec, serviceIdVal, method),
  };
}
