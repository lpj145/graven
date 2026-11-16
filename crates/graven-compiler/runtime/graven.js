/**
 * Graven runtime for JavaScript (format v1 and RPC frames v1).
 *
 * No dependencies, ES2022, for browsers, Node (>= 20) and Deno. This file is the source:
 * `scripts/ts.sh` compiles it with `tsc` into `graven.js` and `graven.d.ts` next to it, which
 * `gravenc gen --lang js` writes into projects.
 */
/** Error thrown when decoding, verifying, or building a Graven message fails. */
export class GravenError extends Error {
    kind;
    field;
    needed;
    expected;
    found;
    constructor(kind, details, message) {
        super(message ?? formatErrorMessage(kind, details));
        this.name = "GravenError";
        this.kind = kind;
        if (details?.field !== undefined)
            this.field = details.field;
        if (details?.needed !== undefined)
            this.needed = details.needed;
        if (details?.expected !== undefined)
            this.expected = details.expected;
        if (details?.found !== undefined)
            this.found = details.found;
    }
}
function formatErrorMessage(kind, details) {
    if (!details)
        return kind;
    if (details.field !== undefined)
        return `${kind}(${details.field})`;
    if (details.needed !== undefined)
        return `${kind}(needed=${details.needed})`;
    if (details.expected !== undefined && details.found !== undefined) {
        return `${kind}(expected=${String(details.expected)}, found=${String(details.found)})`;
    }
    if (details.found !== undefined)
        return `${kind}(${String(details.found)})`;
    return kind;
}
// ---- Little-endian codecs & UTF-8 ----
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const UTF8_DECODER_LOOSE = new TextDecoder("utf-8", { fatal: false, ignoreBOM: true });
export const EMPTY_BYTES = new Uint8Array(0);
export const leU8 = {
    size: 1,
    read: (dv, off) => dv.getUint8(off),
    write: (dv, off, v) => dv.setUint8(off, v),
};
export const leU16 = {
    size: 2,
    read: (dv, off) => dv.getUint16(off, true),
    write: (dv, off, v) => dv.setUint16(off, v, true),
};
export const leU32 = {
    size: 4,
    read: (dv, off) => dv.getUint32(off, true),
    write: (dv, off, v) => dv.setUint32(off, v, true),
};
export const leU64 = {
    size: 8,
    read: (dv, off) => dv.getBigUint64(off, true),
    write: (dv, off, v) => dv.setBigUint64(off, v, true),
};
export const leU128 = {
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
export const leI8 = {
    size: 1,
    read: (dv, off) => dv.getInt8(off),
    write: (dv, off, v) => dv.setInt8(off, v),
};
export const leI16 = {
    size: 2,
    read: (dv, off) => dv.getInt16(off, true),
    write: (dv, off, v) => dv.setInt16(off, v, true),
};
export const leI32 = {
    size: 4,
    read: (dv, off) => dv.getInt32(off, true),
    write: (dv, off, v) => dv.setInt32(off, v, true),
};
export const leI64 = {
    size: 8,
    read: (dv, off) => dv.getBigInt64(off, true),
    write: (dv, off, v) => dv.setBigInt64(off, v, true),
};
export const leI128 = {
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
export const leF32 = {
    size: 4,
    read: (dv, off) => dv.getFloat32(off, true),
    write: (dv, off, v) => dv.setFloat32(off, v, true),
};
export const leF64 = {
    size: 8,
    read: (dv, off) => dv.getFloat64(off, true),
    write: (dv, off, v) => dv.setFloat64(off, v, true),
};
/** 1-byte boolean representation used in `[bool]` and inside `struct` definitions. */
export const leBool = {
    size: 1,
    read: (dv, off) => dv.getUint8(off) !== 0,
    write: (dv, off, v) => dv.setUint8(off, v ? 1 : 0),
};
function viewOf(buf) {
    return new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
}
function readU16At(buf, dv, pos) {
    if (pos < 0 || pos + 2 > buf.length) {
        throw new GravenError("OutOfBounds", { needed: pos + 2 });
    }
    return dv.getUint16(pos, true);
}
function readU32At(buf, dv, pos) {
    if (pos < 0 || pos + 4 > buf.length) {
        throw new GravenError("OutOfBounds", { needed: pos + 4 });
    }
    return dv.getUint32(pos, true);
}
function resolvePtrAt(buf, dv, slotAbs) {
    const rel = dv.getInt32(slotAbs, true);
    if (rel === 0)
        return undefined;
    const target = slotAbs + rel;
    if (target < 0 || target >= buf.length) {
        throw new GravenError("BadOffset");
    }
    return target;
}
function bytesAt(buf, dv, pos) {
    const len = readU32At(buf, dv, pos);
    const end = pos + 4 + len;
    if (end > buf.length) {
        throw new GravenError("OutOfBounds", { needed: end });
    }
    return buf.subarray(pos + 4, end);
}
function decodeUtf8(bytes) {
    try {
        return UTF8_DECODER.decode(bytes);
    }
    catch {
        throw new GravenError("InvalidUtf8");
    }
}
function countedAt(buf, dv, pos, elemSize) {
    const len = readU32At(buf, dv, pos);
    const data = pos + 4;
    const end = data + len * elemSize;
    if (end > buf.length) {
        throw new GravenError("OutOfBounds", { needed: end });
    }
    return { len, data };
}
/** Low-level zero-copy cursor over a serialized Graven table. */
export class TableRef {
    buf;
    dv;
    start;
    fixedLen;
    constructor(buf, dv, start, fixedLen) {
        this.buf = buf;
        this.dv = dv;
        this.start = start;
        this.fixedLen = fixedLen;
    }
    /** Table rooted at byte position `pos`: `[u16 fixed_len][fixed_part]`. */
    static at(buf, pos = 0, dv) {
        const view = dv ?? viewOf(buf);
        const fixedLen = readU16At(buf, view, pos);
        const start = pos + 2;
        if (start + fixedLen > buf.length) {
            throw new GravenError("OutOfBounds", { needed: start + fixedLen });
        }
        return new TableRef(buf, view, start, fixedLen);
    }
    /** Inline table element (no `u16` length prefix; length comes from the enclosing `TableList`). */
    static inline(buf, start, fixedLen, dv) {
        if (start < 0 || start + fixedLen > buf.length) {
            throw new GravenError("OutOfBounds", { needed: start + fixedLen });
        }
        return new TableRef(buf, dv ?? viewOf(buf), start, fixedLen);
    }
    /** Reads bit `b`, returning `false` when beyond `fixedLen` (older writer). */
    bit(b) {
        if (b.byte >= this.fixedLen)
            return false;
        const byteVal = this.buf[this.start + b.byte];
        return byteVal !== undefined && ((byteVal >> b.shift) & 1) !== 0;
    }
    /** Raw byte at `byte` within `fixedLen` (caller guarantees `byte < fixedLen`). */
    fixedByte(byte) {
        return this.buf[this.start + byte] ?? 0;
    }
    /**
     * Checks slot bounds relative to `fixedLen`:
     * - `true`: completely inside `fixedLen`
     * - `false`: starts at or beyond `fixedLen` (evolved older writer)
     * - throws `Truncated`: slot is partially covered by `fixedLen`
     */
    slotState(off, size) {
        if (size === 0)
            return off <= this.fixedLen;
        if (off >= this.fixedLen)
            return false;
        if (off + size > this.fixedLen) {
            throw new GravenError("Truncated", { needed: off + size });
        }
        return true;
    }
    rawU16(off) {
        if (off + 2 > this.fixedLen)
            return 0;
        return this.dv.getUint16(this.start + off, true);
    }
    rawI32(off) {
        if (off + 4 > this.fixedLen)
            return 0;
        return this.dv.getInt32(this.start + off, true);
    }
    /** Reads a fixed-size slot without checking presence bits; returns `fallback` if out of `fixedLen`. */
    raw(off, codec, fallback) {
        if (off + codec.size > this.fixedLen)
            return fallback;
        return codec.read(this.dv, this.start + off);
    }
    /** Reads a `Scalar`, `Enum`, or `Struct` field; returns `undefined` when absent. */
    scalar(off, presence, codec) {
        if (!this.slotState(off, codec.size) || !this.bit(presence))
            return undefined;
        return codec.read(this.dv, this.start + off);
    }
    /** Reads a fixed-size field, returning `defaultValue` when absent (even if slot is out of `fixedLen`). */
    scalarOr(off, presence, codec, defaultValue) {
        if (off + codec.size <= this.fixedLen && this.bit(presence)) {
            return codec.read(this.dv, this.start + off);
        }
        return defaultValue;
    }
    /** Reads a 2-bit table boolean field; returns `undefined` when absent. */
    bool(presence) {
        if (!this.bit(presence))
            return undefined;
        return this.bit({ byte: presence.byte, shift: presence.shift + 1 });
    }
    /** Resolves an `i32` relative pointer slot; returns `undefined` if outside `fixedLen` or `0`. */
    ptr(off) {
        if (!this.slotState(off, 4))
            return undefined;
        return resolvePtrAt(this.buf, this.dv, this.start + off);
    }
    bytes(off) {
        const pos = this.ptr(off);
        return pos === undefined ? undefined : bytesAt(this.buf, this.dv, pos);
    }
    str(off) {
        const b = this.bytes(off);
        return b === undefined ? undefined : decodeUtf8(b);
    }
    /** Pre-verified UTF-8 read (for `*Verified` views after `verify()`). */
    strUnchecked(off) {
        const b = this.bytes(off);
        return b === undefined ? undefined : UTF8_DECODER_LOOSE.decode(b);
    }
    table(off) {
        const pos = this.ptr(off);
        return pos === undefined ? undefined : TableRef.at(this.buf, pos, this.dv);
    }
    union(off) {
        if (!this.slotState(off, 6))
            return undefined;
        const tag = this.dv.getUint16(this.start + off, true);
        if (tag === 0)
            return undefined;
        const pos = resolvePtrAt(this.buf, this.dv, this.start + off + 2);
        if (pos === undefined)
            return undefined;
        return { tag, table: TableRef.at(this.buf, pos, this.dv) };
    }
    scalars(off, codec) {
        const pos = this.ptr(off);
        if (pos === undefined)
            return undefined;
        const { len, data } = countedAt(this.buf, this.dv, pos, codec.size);
        return new ScalarList(this.buf, this.dv, data, len, codec);
    }
    tables(off) {
        const pos = this.ptr(off);
        if (pos === undefined)
            return undefined;
        const len = readU32At(this.buf, this.dv, pos);
        const elemFixedLen = readU16At(this.buf, this.dv, pos + 4);
        const first = pos + 6;
        const end = first + len * elemFixedLen;
        if (end > this.buf.length) {
            throw new GravenError("OutOfBounds", { needed: end });
        }
        return new TableList(this.buf, this.dv, first, len, elemFixedLen);
    }
    strs(off) {
        const pos = this.ptr(off);
        if (pos === undefined)
            return undefined;
        const { len, data } = countedAt(this.buf, this.dv, pos, 4);
        return new StrList(this.buf, this.dv, data, len);
    }
    bytesList(off) {
        const s = this.strs(off);
        return s === undefined ? undefined : new BytesList(s);
    }
}
/** View over a serialized `[T]` list of fixed-size scalars, bools, enums, or structs. */
export class ScalarList {
    buf;
    dv;
    data;
    length;
    codec;
    constructor(buf, dv, data, length, codec) {
        this.buf = buf;
        this.dv = dv;
        this.data = data;
        this.length = length;
        this.codec = codec;
    }
    get(index) {
        if (index < 0 || index >= this.length)
            return undefined;
        return this.codec.read(this.dv, this.data + index * this.codec.size);
    }
    asBytes() {
        return this.buf.subarray(this.data, this.data + this.length * this.codec.size);
    }
    *[Symbol.iterator]() {
        for (let i = 0; i < this.length; i++) {
            yield this.codec.read(this.dv, this.data + i * this.codec.size);
        }
    }
}
/** Low-level view over `[Table]` fixed parts. */
export class TableList {
    buf;
    dv;
    first;
    length;
    elemFixedLen;
    constructor(buf, dv, first, length, elemFixedLen) {
        this.buf = buf;
        this.dv = dv;
        this.first = first;
        this.length = length;
        this.elemFixedLen = elemFixedLen;
    }
    get(index) {
        if (index < 0 || index >= this.length) {
            throw new GravenError("OutOfBounds");
        }
        return TableRef.inline(this.buf, this.first + index * this.elemFixedLen, this.elemFixedLen, this.dv);
    }
    *[Symbol.iterator]() {
        for (let i = 0; i < this.length; i++) {
            yield TableRef.inline(this.buf, this.first + i * this.elemFixedLen, this.elemFixedLen, this.dv);
        }
    }
}
/** View over `[string]` or `[bytes]`. Throws `OutOfBounds`, `BadOffset`, or `InvalidUtf8` on element access. */
export class StrList {
    buf;
    dv;
    slots;
    length;
    constructor(buf, dv, slots, length) {
        this.buf = buf;
        this.dv = dv;
        this.slots = slots;
        this.length = length;
    }
    getBytes(index) {
        if (index < 0 || index >= this.length) {
            throw new GravenError("OutOfBounds");
        }
        const pos = resolvePtrAt(this.buf, this.dv, this.slots + index * 4);
        if (pos === undefined)
            return new Uint8Array(0);
        return bytesAt(this.buf, this.dv, pos);
    }
    get(index) {
        return decodeUtf8(this.getBytes(index));
    }
    *[Symbol.iterator]() {
        for (let i = 0; i < this.length; i++) {
            yield this.get(i);
        }
    }
}
/** View over `[bytes]`, yielding `Uint8Array` slices. */
export class BytesList {
    _inner;
    constructor(inner) {
        this._inner = inner;
    }
    get length() {
        return this._inner.length;
    }
    get(index) {
        return this._inner.getBytes(index);
    }
    *[Symbol.iterator]() {
        for (let i = 0; i < this._inner.length; i++) {
            yield this._inner.getBytes(i);
        }
    }
}
/** Pre-verified `[string]` view (skips re-validating UTF-8). */
export class VStrList {
    inner;
    constructor(inner) {
        this.inner = inner;
    }
    get length() {
        return this.inner.length;
    }
    getBytes(index) {
        return this.inner.getBytes(index);
    }
    get(index) {
        return UTF8_DECODER_LOOSE.decode(this.inner.getBytes(index));
    }
    *[Symbol.iterator]() {
        for (let i = 0; i < this.inner.length; i++) {
            yield this.get(i);
        }
    }
}
// ---- Verification context ----
export const DEFAULT_MAX_DEPTH = 64;
/** Stateful recursion depth and byte-budget tracker (spec §7). */
export class VerifyCtx {
    maxDepth;
    budget;
    depth = 0;
    remaining;
    constructor(bufLen, maxDepth = DEFAULT_MAX_DEPTH, maxBudget) {
        this.maxDepth = maxDepth;
        this.budget = maxBudget ?? Math.max(bufLen * 8, 1 << 20);
        this.remaining = this.budget;
    }
    enter() {
        if (this.depth >= this.maxDepth) {
            throw new GravenError("DepthExceeded");
        }
        this.depth++;
    }
    leave() {
        this.depth--;
    }
    charge(bytes) {
        if (bytes > this.remaining) {
            throw new GravenError("BudgetExceeded");
        }
        this.remaining -= bytes;
    }
    get currentDepth() {
        return this.depth;
    }
    get budgetRemaining() {
        return this.remaining;
    }
}
export function checkRequired(t, reqs) {
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
export function checkRequiredFast(t, mask, reqs) {
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
        if (ok)
            return;
    }
    checkRequired(t, reqs);
}
/** Typed list of verified or checked table readers; an absent list reads as empty. */
export class TableViewList {
    inner;
    wrap;
    constructor(inner, wrap) {
        this.inner = inner;
        this.wrap = wrap;
    }
    get length() {
        return this.inner === undefined ? 0 : this.inner.length;
    }
    get(index) {
        if (this.inner === undefined) {
            throw new GravenError("OutOfBounds");
        }
        return this.wrap(this.inner.get(index));
    }
    *[Symbol.iterator]() {
        if (this.inner === undefined)
            return;
        for (let i = 0; i < this.inner.length; i++) {
            yield this.wrap(this.inner.get(i));
        }
    }
}
export function verifyStrs(list, ctx) {
    if (list === undefined)
        return;
    ctx.charge(4 + list.length * 4);
    for (let i = 0; i < list.length; i++) {
        const s = list.getBytes(i);
        decodeUtf8(s);
        ctx.charge(4 + s.length);
    }
}
export function verifyBytesList(list, ctx) {
    if (list === undefined)
        return;
    ctx.charge(4 + list.length * 4);
    for (let i = 0; i < list.length; i++) {
        const b = list.getBytes(i);
        ctx.charge(4 + b.length);
    }
}
export function verifyTableList(list, fromRef, ctx) {
    if (list === undefined)
        return;
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
export function crc32c(data) {
    let crc = 0xffffffff;
    for (let i = 0; i < data.length; i++) {
        crc = (CRC32C_TABLE[(crc ^ data[i]) & 0xff] ^ (crc >>> 8)) >>> 0;
    }
    return (crc ^ 0xffffffff) >>> 0;
}
const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const U64_MASK = 0xffffffffffffffffn;
const UTF8_ENCODER = new TextEncoder();
/** FNV-1a 64-bit hash of `"namespace.Type"` or `"namespace.Service"` (spec §9.2). */
export function schemaId(qualifiedName) {
    const bytes = UTF8_ENCODER.encode(qualifiedName);
    let hash = FNV_OFFSET;
    for (let i = 0; i < bytes.length; i++) {
        hash ^= BigInt(bytes[i]);
        hash = (hash * FNV_PRIME) & U64_MASK;
    }
    return hash;
}
export const ENVELOPE_HEADER_LEN = 20;
export const ENVELOPE_TRAILER_LEN = 4;
const ENVELOPE_VERSION = 1;
const I32_MAX = 0x7fff_ffff;
/** Writes the 20-byte envelope header at `out[offset..offset+20]`. */
export function writeEnvelopeHeader(out, offset, id, payloadLen) {
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
export function encodeEnvelope(id, payload) {
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
export function decodeEnvelope(buf) {
    if (buf.length < ENVELOPE_HEADER_LEN) {
        throw new GravenError("Incomplete", { needed: ENVELOPE_HEADER_LEN });
    }
    if (buf[0] !== 0x47 || buf[1] !== 0x52 || buf[2] !== 0x56 || buf[3] !== 0x4e) {
        throw new GravenError("BadMagic");
    }
    const version = buf[4];
    if (version !== ENVELOPE_VERSION) {
        throw new GravenError("UnsupportedVersion", { found: version });
    }
    const dv = viewOf(buf);
    const flags = buf[5];
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
export function* envelopes(buf) {
    let offset = 0;
    while (offset < buf.length) {
        const env = decodeEnvelope(buf.subarray(offset));
        offset += env.totalLen;
        yield env;
    }
}
/** Decodes an envelope and validates that its `schemaId` matches `expected`. */
export function openEnvelope(buf, expected) {
    const env = decodeEnvelope(buf);
    if (env.schemaId !== expected) {
        throw new GravenError("SchemaMismatch", { expected, found: env.schemaId });
    }
    return env.payload;
}
// ---- Canonical writer (spec §3–§6) ----
class GrowBuf {
    buf;
    dv;
    len = 0;
    constructor(initialCapacity = 256) {
        const cap = Math.max(initialCapacity, 64);
        this.buf = new Uint8Array(cap);
        this.dv = new DataView(this.buf.buffer);
    }
    ensure(extra) {
        const needed = this.len + extra;
        if (needed <= this.buf.length)
            return;
        let nextCap = this.buf.length * 2;
        while (nextCap < needed)
            nextCap *= 2;
        const next = new Uint8Array(nextCap);
        next.set(this.buf.subarray(0, this.len));
        this.buf = next;
        this.dv = new DataView(this.buf.buffer);
    }
    allocZero(size) {
        this.ensure(size);
        const pos = this.len;
        this.buf.fill(0, pos, pos + size);
        this.len += size;
        return pos;
    }
    pushBytes(bytes) {
        this.ensure(bytes.length);
        this.buf.set(bytes, this.len);
        this.len += bytes.length;
    }
    pushU32(value) {
        const pos = this.allocZero(4);
        this.dv.setUint32(pos, value, true);
    }
    pushU16(value) {
        const pos = this.allocZero(2);
        this.dv.setUint16(pos, value, true);
    }
    copySlice(start, end) {
        return this.buf.slice(start, end);
    }
    clear() {
        this.len = 0;
    }
}
function allocTableW(main, fixedLen) {
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
    main;
    sealed = 0;
    msgStart;
    sealMode;
    pendingClose = [];
    constructor(initialCapacity = 256) {
        this.main = new GrowBuf(initialCapacity);
    }
    root(fixedLen) {
        this.pendingClose = [];
        this.main.len = this.sealed;
        const prefix = this.sealMode === "frame" ? 4 : ENVELOPE_HEADER_LEN;
        this.main.allocZero(prefix);
        const ms = this.main.len;
        this.msgStart = ms;
        const start = allocTableW(this.main, fixedLen);
        return new TableW(this.main, undefined, start, this.pendingClose);
    }
    flushPending() {
        while (this.pendingClose.length > 0) {
            this.pendingClose.pop()();
        }
    }
    /** Returns the current bare message (throws `TooLarge` if > `i32::MAX`). */
    finish() {
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
    finishEnvelope(id) {
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
    seal() {
        if (this.sealMode === "envelope") {
            throw new Error("builder mode mismatch: already sealing envelopes");
        }
        this.flushPending();
        const ms = this.msgStart;
        this.msgStart = undefined;
        if (ms === undefined)
            return;
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
    sealEnvelope(id) {
        if (this.sealMode === "frame") {
            throw new Error("builder mode mismatch: already sealing length-prefixed frames");
        }
        this.flushPending();
        const ms = this.msgStart;
        this.msgStart = undefined;
        if (ms === undefined)
            return;
        const payloadLen = this.main.len - ms;
        if (payloadLen > I32_MAX) {
            this.main.len = this.sealed;
            throw new GravenError("TooLarge");
        }
        this.sealMode = "envelope";
        this.closeEnvelope(ms, id, payloadLen);
        this.sealed = this.main.len;
    }
    get sealedLen() {
        return this.sealed;
    }
    takeFrames() {
        const out = this.main.copySlice(0, this.sealed);
        this.main.clear();
        this.sealed = 0;
        this.msgStart = undefined;
        this.sealMode = undefined;
        return out;
    }
    closeEnvelope(ms, id, payloadLen) {
        const hdrStart = ms - ENVELOPE_HEADER_LEN;
        writeEnvelopeHeader(this.main.buf, hdrStart, id, payloadLen);
        const crc = crc32c(this.main.buf.subarray(hdrStart, this.main.len));
        this.main.pushU32(crc);
    }
}
/** Writer for one table's fixed and variable fields. */
export class TableW {
    main;
    aux;
    start;
    pendingClose;
    constructor(main, aux, start, pendingClose = []) {
        this.main = main;
        this.aux = aux;
        this.start = start;
        this.pendingClose = pendingClose;
    }
    fixedTarget() {
        return this.aux ?? this.main;
    }
    setBit(b) {
        const t = this.fixedTarget();
        const idx = this.start + b.byte;
        t.buf[idx] = (t.buf[idx] ?? 0) | (1 << b.shift);
    }
    clearBit(b) {
        const t = this.fixedTarget();
        const idx = this.start + b.byte;
        t.buf[idx] = (t.buf[idx] ?? 0) & ~(1 << b.shift);
    }
    patch(off, target) {
        const slot = this.start + off;
        const t = this.fixedTarget();
        const val = this.aux !== undefined ? target + 1 : target - slot;
        t.dv.setInt32(slot, val, true);
    }
    set(off, presence, codec, value) {
        const t = this.fixedTarget();
        if (codec.size > 0) {
            codec.write(t.dv, this.start + off, value);
        }
        this.setBit(presence);
        return this;
    }
    setBool(presence, value) {
        this.setBit(presence);
        const valBit = { byte: presence.byte, shift: presence.shift + 1 };
        if (value)
            this.setBit(valBit);
        else
            this.clearBit(valBit);
        return this;
    }
    setBytes(off, bytes) {
        const pos = this.main.len;
        this.main.pushU32(bytes.length);
        this.main.pushBytes(bytes);
        this.patch(off, pos);
        return this;
    }
    setStr(off, s) {
        return this.setBytes(off, UTF8_ENCODER.encode(s));
    }
    setScalars(off, codec, values) {
        const items = Array.isArray(values) ? values : [...values];
        const pos = this.main.len;
        this.main.allocZero(4 + items.length * codec.size);
        this.main.dv.setUint32(pos, items.length, true);
        if (codec.size > 0) {
            let cur = pos + 4;
            for (let i = 0; i < items.length; i++) {
                codec.write(this.main.dv, cur, items[i]);
                cur += codec.size;
            }
        }
        this.patch(off, pos);
        return this;
    }
    setStrs(off, values) {
        const items = [...values].map((s) => UTF8_ENCODER.encode(s));
        return this.setBytesList(off, items);
    }
    setBytesList(off, values) {
        const items = Array.isArray(values) ? values : [...values];
        const pos = this.main.len;
        this.main.allocZero(4 + 4 * items.length);
        this.main.dv.setUint32(pos, items.length, true);
        for (let i = 0; i < items.length; i++) {
            const slot = pos + 4 + 4 * i;
            const target = this.main.len;
            const b = items[i];
            this.main.pushU32(b.length);
            this.main.pushBytes(b);
            this.main.dv.setInt32(slot, target - slot, true);
        }
        this.patch(off, pos);
        return this;
    }
    table(off, fixedLen) {
        const pos = this.main.len;
        const start = allocTableW(this.main, fixedLen);
        this.patch(off, pos);
        return new TableW(this.main, undefined, start, this.pendingClose);
    }
    union(off, tag, fixedLen) {
        if (tag === 0) {
            throw new Error("union tag 0 is reserved for absent");
        }
        const t = this.fixedTarget();
        t.dv.setUint16(this.start + off, tag, true);
        return this.table(off + 2, fixedLen);
    }
    tables(off, count, elemFixedLen) {
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
    tablesStreaming(off, elemFixedLen, ptrSlots) {
        const list = new StreamListW(this, off, elemFixedLen, ptrSlots);
        this.pendingClose.push(() => list.close());
        return list;
    }
    scalarsStreaming(off, codec) {
        const pos = this.main.len;
        this.main.allocZero(4);
        this.patch(off, pos);
        return new ScalarListW(this.main, pos, codec);
    }
}
/** Fixed-count list-of-tables writer; updates count on every `push()`. */
export class TableListW {
    main;
    pos;
    first;
    efl;
    cap;
    pendingClose;
    count = 0;
    constructor(main, pos, first, efl, cap, pendingClose = []) {
        this.main = main;
        this.pos = pos;
        this.first = first;
        this.efl = efl;
        this.cap = cap;
        this.pendingClose = pendingClose;
    }
    push() {
        if (this.count >= this.cap) {
            throw new Error("TableListW: pushed more elements than reserved");
        }
        const start = this.first + this.count * this.efl;
        this.count++;
        this.main.dv.setUint32(this.pos, this.count, true);
        return new TableW(this.main, undefined, start, this.pendingClose);
    }
    close() {
        this.main.dv.setUint32(this.pos, this.count, true);
    }
}
/** Streaming list-of-tables writer; finalizes on `close()`. */
export class StreamListW {
    parent;
    off;
    efl;
    ptrSlots;
    aux = new GrowBuf(128);
    count = 0;
    closed = false;
    constructor(parent, off, efl, ptrSlots) {
        this.parent = parent;
        this.off = off;
        this.efl = efl;
        this.ptrSlots = ptrSlots;
    }
    push() {
        if (this.closed) {
            throw new Error("StreamListW: already closed");
        }
        const start = this.aux.allocZero(this.efl);
        this.count++;
        return new TableW(this.parent.main, this.aux, start, this.parent.pendingClose);
    }
    close() {
        if (this.closed)
            return;
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
export class ScalarListW {
    main;
    pos;
    codec;
    count = 0;
    constructor(main, pos, codec) {
        this.main = main;
        this.pos = pos;
        this.codec = codec;
    }
    push(value) {
        if (this.codec.size > 0) {
            const off = this.main.allocZero(this.codec.size);
            this.codec.write(this.main.dv, off, value);
        }
        this.count++;
        this.main.dv.setUint32(this.pos, this.count, true);
        return this;
    }
    close() {
        this.main.dv.setUint32(this.pos, this.count, true);
    }
}
// ---- Typed generated-code wrappers & validation ----
export function utf8Len(s) {
    return UTF8_ENCODER.encode(s).length;
}
export function reqPtr(val, field) {
    if (val === undefined) {
        throw new GravenError("Missing", { field });
    }
    return val;
}
export function optVStrs(s) {
    return s === undefined ? undefined : new VStrList(s);
}
export function reqVStrs(s, field) {
    return new VStrList(reqPtr(s, field));
}
/** Typed fixed-count list writer returning typed element writers `W`. */
export class ListW {
    inner;
    wrap;
    constructor(inner, wrap) {
        this.inner = inner;
        this.wrap = wrap;
    }
    push() {
        return this.wrap(this.inner.push());
    }
    close() {
        this.inner.close();
    }
}
/** Typed streaming list writer returning typed element writers `W`. */
export class StreamW {
    inner;
    wrap;
    constructor(inner, wrap) {
        this.inner = inner;
        this.wrap = wrap;
    }
    push() {
        return this.wrap(this.inner.push());
    }
    close() {
        this.inner.close();
    }
}
/** Adds the code a check returned (`r`) to a failure; used by generated code. */
export function checkFailure(f, r) {
    return typeof r === "string" ? { ...f, reason: r } : f;
}
/** Thrown by `validate()` when a schema validation rule fails. */
export class ValidationError extends Error {
    failure;
    constructor(failure) {
        const loc = failure.field !== undefined
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
};
export const FaultCode = {
    Unsupported: 1,
    Malformed: 2,
    Invalid: 3,
    DeadlineExceeded: 4,
    Internal: 5,
    Unavailable: 6,
    isKnown(code) {
        return code >= 1 && code <= 6;
    },
    normalize(code) {
        return code >= 1 && code <= 6 ? code : 5;
    },
};
export const BROKEN_RULE_SCHEMA_ID = schemaId("graven.rpc.BrokenRule");
export const FAULT_SCHEMA_ID = schemaId("graven.rpc.Fault");
const FAULT_FIXED_LEN = 11;
/** `BrokenRule` up to `rule` (@3): written when `min`, `max` and `reason` are absent. */
const BROKEN_RULE_FIXED_LEN_V0 = 17;
/** `BrokenRule` with `min`, `max` and `reason` (@4–@6). */
const BROKEN_RULE_FIXED_LEN = 29;
function writeFrameHeader(out, header) {
    const dv = new DataView(out.buffer, out.byteOffset, FRAME_HEADER_LEN);
    out[0] = FRAME_VERSION;
    out[1] = header.kind;
    dv.setUint16(2, header.method, true);
    dv.setUint16(4, header.variant, true);
    dv.setUint16(6, 0, true);
    dv.setBigUint64(8, header.serviceId, true);
}
/** Encodes a single RPC v1 frame. */
export function encodeFrame(header, schemaIdVal, payload) {
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
export function encodeEnd(serviceIdVal, method) {
    return encodeFrame({ kind: FrameKind.End, method, variant: 0, serviceId: serviceIdVal }, 0n, EMPTY_BYTES);
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
export function decodeFrame(buf) {
    if (buf.length < FRAME_HEADER_LEN) {
        throw new GravenError("Incomplete", { needed: FRAME_HEADER_LEN });
    }
    const version = buf[0];
    if (version !== FRAME_VERSION) {
        throw new GravenError("UnsupportedFrameVersion", { found: version });
    }
    const kindByte = buf[1];
    if (kindByte < 1 || kindByte > 6) {
        throw new GravenError("BadKind", { found: kindByte });
    }
    const kind = kindByte;
    const dv = new DataView(buf.buffer, buf.byteOffset, FRAME_HEADER_LEN);
    const method = dv.getUint16(2, true);
    const variant = dv.getUint16(4, true);
    const reserved = dv.getUint16(6, true);
    const serviceIdVal = dv.getBigUint64(8, true);
    if (reserved !== 0 || (kind === FrameKind.End && variant !== 0)) {
        throw new GravenError("BadFrameHeader");
    }
    const header = { kind, method, variant, serviceId: serviceIdVal };
    if (kind === FrameKind.End) {
        return { header, schemaId: 0n, payload: EMPTY_BYTES, totalLen: FRAME_HEADER_LEN };
    }
    let env;
    try {
        env = decodeEnvelope(buf.subarray(FRAME_HEADER_LEN));
    }
    catch (e) {
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
export function* frames(buf) {
    let off = 0;
    while (off < buf.length) {
        const f = decodeFrame(buf.subarray(off));
        yield f;
        off += f.totalLen;
    }
}
/** Verifies and decodes a `graven.rpc.Fault` message payload. */
export function decodeFault(payload) {
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
    let broken;
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
        const opt = (off) => {
            const s = b.str(off);
            if (s !== undefined)
                ctx.charge(4 + utf8Len(s));
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
export function encodeFaultPayload(fault) {
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
        if (br.min !== undefined)
            bw.setStr(17, br.min);
        if (br.max !== undefined)
            bw.setStr(21, br.max);
        if (br.reason !== undefined)
            bw.setStr(25, br.reason);
    }
    return b.finish();
}
/** Encodes a full `Fault` RPC frame. */
export function encodeFaultFrame(serviceIdVal, method, fault) {
    return encodeFrame({ kind: FrameKind.Fault, method, variant: 0, serviceId: serviceIdVal }, FAULT_SCHEMA_ID, encodeFaultPayload(fault));
}
/** Error thrown when the server replies with a `Fault` frame. */
export class RpcFault extends Error {
    code;
    faultMessage;
    broken;
    constructor(fault) {
        super(fault.message ?? `rpc fault code=${fault.code}`);
        this.name = "RpcFault";
        this.code = fault.code;
        if (fault.message !== undefined)
            this.faultMessage = fault.message;
        if (fault.broken !== undefined)
            this.broken = fault.broken;
    }
}
/** Error thrown when a reply violates the Graven RPC v1 protocol. */
export class RpcProtocolError extends Error {
    kind;
    causeError;
    frameKind;
    expected;
    found;
    variant;
    trailing;
    constructor(kind, details) {
        super(formatRpcProtocolError(kind, details));
        this.name = "RpcProtocolError";
        this.kind = kind;
        if (details?.causeError !== undefined)
            this.causeError = details.causeError;
        if (details?.frameKind !== undefined)
            this.frameKind = details.frameKind;
        if (details?.expected !== undefined)
            this.expected = details.expected;
        if (details?.found !== undefined)
            this.found = details.found;
        if (details?.variant !== undefined)
            this.variant = details.variant;
        if (details?.trailing !== undefined)
            this.trailing = details.trailing;
    }
}
function formatRpcProtocolError(kind, d) {
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
export function rpcVerifyTable(variant, foundSchemaId, payload, expectedSchemaId, verifyFn) {
    if (variant !== 0) {
        throw new RpcProtocolError("UnexpectedVariant", { variant });
    }
    if (foundSchemaId !== expectedSchemaId) {
        throw new RpcProtocolError("SchemaMismatch", { expected: expectedSchemaId, found: foundSchemaId });
    }
    try {
        return verifyFn(payload);
    }
    catch (e) {
        if (e instanceof GravenError) {
            throw new RpcProtocolError("Payload", { causeError: e });
        }
        throw e;
    }
}
export function encodeCallFrame(codec, serviceIdVal, method, fn) {
    const { variant, schemaId: sid, payload } = codec.encode(fn);
    return encodeFrame({ kind: FrameKind.Call, method, variant, serviceId: serviceIdVal }, sid, payload);
}
function isReadableStream(v) {
    return typeof v.getReader === "function";
}
class ChunkSource {
    streamReader;
    iter;
    initial;
    done = false;
    constructor(input) {
        if (input instanceof Uint8Array) {
            this.initial = input;
            if (input.length === 0)
                this.done = true;
        }
        else if (isReadableStream(input)) {
            this.streamReader = input.getReader();
        }
        else {
            this.iter = input[Symbol.asyncIterator]();
        }
    }
    async next() {
        if (this.initial !== undefined) {
            const c = this.initial;
            this.initial = undefined;
            this.done = true;
            return c.length > 0 ? c : undefined;
        }
        if (this.done)
            return undefined;
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
    async close() {
        if (this.streamReader !== undefined) {
            try {
                if (!this.done)
                    await this.streamReader.cancel();
            }
            catch {
                // ignore cancel errors during cleanup
            }
            finally {
                this.streamReader.releaseLock();
            }
        }
        else if (this.iter !== undefined && !this.done && typeof this.iter.return === "function") {
            try {
                await this.iter.return();
            }
            catch {
                // ignore return errors during cleanup
            }
        }
        this.done = true;
    }
}
async function collectReplyBytes(input) {
    if (input instanceof Uint8Array)
        return input;
    const src = new ChunkSource(input);
    const chunks = [];
    let total = 0;
    try {
        while (true) {
            const c = await src.next();
            if (c === undefined)
                break;
            if (c.length > 0) {
                chunks.push(c);
                total += c.length;
            }
        }
    }
    finally {
        await src.close();
    }
    if (chunks.length === 0)
        return EMPTY_BYTES;
    if (chunks.length === 1)
        return chunks[0];
    const out = new Uint8Array(total);
    let pos = 0;
    for (const c of chunks) {
        out.set(c, pos);
        pos += c.length;
    }
    return out;
}
class FrameStreamReader {
    src;
    buf = new Uint8Array(0);
    pos = 0;
    len = 0;
    eof = false;
    constructor(input) {
        this.src = new ChunkSource(input);
    }
    trailingCount() {
        return this.len - this.pos;
    }
    append(chunk) {
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
        while (nextCap < avail + chunk.length)
            nextCap *= 2;
        const grown = new Uint8Array(nextCap);
        if (avail > 0)
            grown.set(this.buf.subarray(this.pos, this.len), 0);
        grown.set(chunk, avail);
        this.buf = grown;
        this.pos = 0;
        this.len = avail + chunk.length;
    }
    async nextFrame() {
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
                }
                catch (e) {
                    if (e instanceof GravenError && e.kind === "Incomplete") {
                        if (this.eof) {
                            throw new RpcProtocolError("Truncated");
                        }
                    }
                    else if (e instanceof GravenError) {
                        throw new RpcProtocolError("Frame", { causeError: e });
                    }
                    else {
                        throw e;
                    }
                }
            }
            else if (this.eof) {
                return undefined;
            }
            const chunk = await this.src.next();
            if (chunk === undefined) {
                this.eof = true;
            }
            else if (chunk.length > 0) {
                this.append(chunk);
            }
        }
    }
    async close() {
        await this.src.close();
    }
}
function checkFrameCall(frame, serviceIdVal, method) {
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
function decodeFaultFrameError(frame) {
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
    }
    catch (e) {
        if (e instanceof GravenError) {
            return new RpcProtocolError("Payload", { causeError: e });
        }
        throw e;
    }
}
function decodeTerminalFrame(bytes, serviceIdVal, method) {
    if (bytes.length === 0) {
        throw new RpcProtocolError("EmptyReply");
    }
    let frame;
    try {
        frame = decodeFrame(bytes);
    }
    catch (e) {
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
export async function rpcOneWay(send, inCodec, serviceIdVal, method, fn, opts) {
    const call = encodeCallFrame(inCodec, serviceIdVal, method, fn);
    const reply = await send(call, opts?.timeoutMs);
    const bytes = await collectReplyBytes(reply);
    if (bytes.length !== 0) {
        throw new RpcProtocolError("UnexpectedOneWayReply");
    }
}
export async function rpcUnary(send, inCodec, outCodec, serviceIdVal, method, defaultTimeoutMs, fn, opts) {
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
export async function rpcUnaryResult(send, inCodec, outCodec, errCodec, serviceIdVal, method, defaultTimeoutMs, fn, opts) {
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
async function* iterateStreamFrames(reader, first, outCodec, serviceIdVal, method) {
    let pending = first;
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
            }
            else if (frame.header.kind === FrameKind.End) {
                const trailing = reader.trailingCount();
                if (trailing > 0) {
                    throw new RpcProtocolError("TrailingBytes", { trailing });
                }
                return;
            }
            else if (frame.header.kind === FrameKind.Fault) {
                const trailing = reader.trailingCount();
                if (trailing > 0) {
                    throw new RpcProtocolError("TrailingBytes", { trailing });
                }
                throw decodeFaultFrameError(frame);
            }
            else {
                throw new RpcProtocolError("UnexpectedKind", { frameKind: frame.header.kind });
            }
        }
    }
    finally {
        await reader.close();
    }
}
async function openStreamReply(input, serviceIdVal, method) {
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
    }
    catch (e) {
        await reader.close();
        throw e;
    }
}
export async function rpcStream(send, inCodec, outCodec, serviceIdVal, method, defaultTimeoutMs, fn, opts) {
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
export async function rpcStreamResult(send, inCodec, outCodec, errCodec, serviceIdVal, method, defaultTimeoutMs, fn, opts) {
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
