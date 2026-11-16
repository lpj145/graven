/**
 * Graven runtime for JavaScript (format v1 and RPC frames v1).
 *
 * No dependencies, ES2022, for browsers, Node (>= 20) and Deno. This file is the source:
 * `scripts/ts.sh` compiles it with `tsc` into `graven.js` and `graven.d.ts` next to it, which
 * `gravenc gen --lang js` writes into projects.
 */
export type GravenErrorKind = "OutOfBounds" | "Truncated" | "BadOffset" | "InvalidUtf8" | "Missing" | "DepthExceeded" | "BudgetExceeded" | "TooLarge" | "BadMagic" | "UnsupportedVersion" | "BadHeader" | "Incomplete" | "SchemaMismatch" | "ChecksumMismatch" | "UnsupportedFrameVersion" | "BadKind" | "BadFrameHeader";
export interface GravenErrorDetails {
    readonly field?: string;
    readonly needed?: number;
    readonly expected?: bigint | number;
    readonly found?: bigint | number;
}
/** Error thrown when decoding, verifying, or building a Graven message fails. */
export declare class GravenError extends Error {
    readonly kind: GravenErrorKind;
    readonly field?: string;
    readonly needed?: number;
    readonly expected?: bigint | number;
    readonly found?: bigint | number;
    constructor(kind: GravenErrorKind, details?: GravenErrorDetails, message?: string);
}
export declare const EMPTY_BYTES: Uint8Array<ArrayBuffer>;
export interface LeReader<T> {
    readonly size: number;
    read(dv: DataView, offset: number): T;
}
export interface LeCodec<T> extends LeReader<T> {
    write(dv: DataView, offset: number, value: T): void;
}
export declare const leU8: LeCodec<number>;
export declare const leU16: LeCodec<number>;
export declare const leU32: LeCodec<number>;
export declare const leU64: LeCodec<bigint>;
export declare const leU128: LeCodec<bigint>;
export declare const leI8: LeCodec<number>;
export declare const leI16: LeCodec<number>;
export declare const leI32: LeCodec<number>;
export declare const leI64: LeCodec<bigint>;
export declare const leI128: LeCodec<bigint>;
export declare const leF32: LeCodec<number>;
export declare const leF64: LeCodec<number>;
/** 1-byte boolean representation used in `[bool]` and inside `struct` definitions. */
export declare const leBool: LeCodec<boolean>;
/** Bit position of a presence (or bool value) bit inside a table's fixed part. */
export interface Bit {
    readonly byte: number;
    readonly shift: number;
}
/** Low-level zero-copy cursor over a serialized Graven table. */
export declare class TableRef {
    readonly buf: Uint8Array;
    readonly dv: DataView;
    readonly start: number;
    readonly fixedLen: number;
    private constructor();
    /** Table rooted at byte position `pos`: `[u16 fixed_len][fixed_part]`. */
    static at(buf: Uint8Array, pos?: number, dv?: DataView): TableRef;
    /** Inline table element (no `u16` length prefix; length comes from the enclosing `TableList`). */
    static inline(buf: Uint8Array, start: number, fixedLen: number, dv?: DataView): TableRef;
    /** Reads bit `b`, returning `false` when beyond `fixedLen` (older writer). */
    bit(b: Bit): boolean;
    /** Raw byte at `byte` within `fixedLen` (caller guarantees `byte < fixedLen`). */
    fixedByte(byte: number): number;
    /**
     * Checks slot bounds relative to `fixedLen`:
     * - `true`: completely inside `fixedLen`
     * - `false`: starts at or beyond `fixedLen` (evolved older writer)
     * - throws `Truncated`: slot is partially covered by `fixedLen`
     */
    slotState(off: number, size: number): boolean;
    rawU16(off: number): number;
    rawI32(off: number): number;
    /** Reads a fixed-size slot without checking presence bits; returns `fallback` if out of `fixedLen`. */
    raw<T>(off: number, codec: LeReader<T>, fallback: T): T;
    /** Reads a `Scalar`, `Enum`, or `Struct` field; returns `undefined` when absent. */
    scalar<T>(off: number, presence: Bit, codec: LeReader<T>): T | undefined;
    /** Reads a fixed-size field, returning `defaultValue` when absent (even if slot is out of `fixedLen`). */
    scalarOr<T>(off: number, presence: Bit, codec: LeReader<T>, defaultValue: T): T;
    /** Reads a 2-bit table boolean field; returns `undefined` when absent. */
    bool(presence: Bit): boolean | undefined;
    /** Resolves an `i32` relative pointer slot; returns `undefined` if outside `fixedLen` or `0`. */
    ptr(off: number): number | undefined;
    bytes(off: number): Uint8Array | undefined;
    str(off: number): string | undefined;
    /** Pre-verified UTF-8 read (for `*Verified` views after `verify()`). */
    strUnchecked(off: number): string | undefined;
    table(off: number): TableRef | undefined;
    union(off: number): {
        readonly tag: number;
        readonly table: TableRef;
    } | undefined;
    scalars<T>(off: number, codec: LeReader<T>): ScalarList<T> | undefined;
    tables(off: number): TableList | undefined;
    strs(off: number): StrList | undefined;
    bytesList(off: number): BytesList | undefined;
    /** Verifies a string slot's bounds and UTF-8 and charges `ctx` without allocating a JS string. */
    verifyStr(off: number, ctx: VerifyCtx): void;
    /** Verifies a bytes slot's bounds and charges `ctx` without allocating a subarray. */
    verifyBytes(off: number, ctx: VerifyCtx): void;
    /** Verifies a scalar/enum/struct list slot's bounds and charges `ctx` without allocating `ScalarList`. */
    verifyScalars(off: number, elemSize: number, ctx: VerifyCtx): void;
}
/** View over a serialized `[T]` list of fixed-size scalars, bools, enums, or structs. */
export declare class ScalarList<T> implements Iterable<T> {
    private readonly buf;
    private readonly dv;
    private readonly data;
    readonly length: number;
    private readonly codec;
    constructor(buf: Uint8Array, dv: DataView, data: number, length: number, codec: LeReader<T>);
    get(index: number): T | undefined;
    asBytes(): Uint8Array;
    [Symbol.iterator](): Iterator<T>;
}
/** Low-level view over `[Table]` fixed parts. */
export declare class TableList implements Iterable<TableRef> {
    private readonly buf;
    private readonly dv;
    private readonly first;
    readonly length: number;
    readonly elemFixedLen: number;
    constructor(buf: Uint8Array, dv: DataView, first: number, length: number, elemFixedLen: number);
    get(index: number): TableRef;
    [Symbol.iterator](): Iterator<TableRef>;
}
/** View over `[string]` or `[bytes]`. Throws `OutOfBounds`, `BadOffset`, or `InvalidUtf8` on element access. */
export declare class StrList implements Iterable<string> {
    private readonly buf;
    private readonly dv;
    private readonly slots;
    readonly length: number;
    constructor(buf: Uint8Array, dv: DataView, slots: number, length: number);
    getBytes(index: number): Uint8Array;
    _verify(ctx: VerifyCtx, checkUtf8: boolean): void;
    get(index: number): string;
    [Symbol.iterator](): Iterator<string>;
}
/** View over `[bytes]`, yielding `Uint8Array` slices. */
export declare class BytesList implements Iterable<Uint8Array> {
    readonly _inner: StrList;
    constructor(inner: StrList);
    get length(): number;
    get(index: number): Uint8Array;
    [Symbol.iterator](): Iterator<Uint8Array>;
}
/** Pre-verified `[string]` view (skips re-validating UTF-8). */
export declare class VStrList implements Iterable<string> {
    private readonly inner;
    constructor(inner: StrList);
    get length(): number;
    getBytes(index: number): Uint8Array;
    get(index: number): string;
    [Symbol.iterator](): Iterator<string>;
}
export declare const DEFAULT_MAX_DEPTH = 64;
/** Stateful recursion depth and byte-budget tracker (spec §7). */
export declare class VerifyCtx {
    readonly maxDepth: number;
    readonly budget: number;
    private depth;
    private remaining;
    constructor(bufLen: number, maxDepth?: number, maxBudget?: number);
    enter(): void;
    leave(): void;
    charge(bytes: number): void;
    get currentDepth(): number;
    get budgetRemaining(): number;
}
export type Req = {
    readonly kind: "slot";
    readonly off: number;
    readonly size: number;
} | {
    readonly kind: "scalar";
    readonly name: string;
    readonly off: number;
    readonly size: number;
    readonly bit: Bit;
} | {
    readonly kind: "bool";
    readonly name: string;
    readonly bit: Bit;
} | {
    readonly kind: "ptr";
    readonly name: string;
    readonly off: number;
} | {
    readonly kind: "union";
    readonly name: string;
    readonly off: number;
};
export interface ReqMask {
    readonly len: number;
    readonly bits: readonly (readonly [number, number])[];
    readonly ptrs: readonly number[];
    readonly unions: readonly number[];
}
export declare function checkRequired(t: TableRef, reqs: readonly Req[]): void;
export declare function checkRequiredFast(t: TableRef, mask: ReqMask, reqs: readonly Req[]): void;
/** Typed list of verified or checked table readers; an absent list reads as empty. */
export declare class TableViewList<T> implements Iterable<T> {
    private readonly inner;
    private readonly wrap;
    constructor(inner: TableList | undefined, wrap: (t: TableRef) => T);
    get length(): number;
    get(index: number): T;
    [Symbol.iterator](): Iterator<T>;
}
export declare function verifyStrs(list: StrList | undefined, ctx: VerifyCtx): void;
export declare function verifyBytesList(list: StrList | undefined, ctx: VerifyCtx): void;
export declare function verifyTableList<T extends {
    _verifyContent(ctx: VerifyCtx): void;
}>(list: TableList | undefined, fromRef: (t: TableRef) => T, ctx: VerifyCtx): void;
/** Computes CRC-32C (Castagnoli) over `data` as an unsigned 32-bit integer. */
export declare function crc32c(data: Uint8Array): number;
/** FNV-1a 64-bit hash of `"namespace.Type"` or `"namespace.Service"` (spec §9.2). */
export declare function schemaId(qualifiedName: string): bigint;
export declare const ENVELOPE_HEADER_LEN = 20;
export declare const ENVELOPE_TRAILER_LEN = 4;
export interface Envelope {
    readonly schemaId: bigint;
    readonly payload: Uint8Array;
    readonly totalLen: number;
}
/** Writes the 20-byte envelope header at `out[offset..offset+20]`. */
export declare function writeEnvelopeHeader(out: Uint8Array, offset: number, id: bigint, payloadLen: number): void;
/** Encodes `payload` inside a GRVN v1 envelope with CRC32C trailer. */
export declare function encodeEnvelope(id: bigint, payload: Uint8Array): Uint8Array;
/** Decodes and verifies a GRVN v1 envelope header and CRC32C checksum. */
export declare function decodeEnvelope(buf: Uint8Array): Envelope;
/** Iterates over concatenated GRVN v1 envelopes in `buf`. */
export declare function envelopes(buf: Uint8Array): Generator<Envelope>;
/** Decodes an envelope and validates that its `schemaId` matches `expected`. */
export declare function openEnvelope(buf: Uint8Array, expected: bigint): Uint8Array;
declare class GrowBuf {
    buf: Uint8Array;
    dv: DataView;
    len: number;
    constructor(initialCapacity?: number);
    ensure(extra: number): void;
    allocZero(size: number): number;
    pushBytes(bytes: Uint8Array): void;
    pushU32(value: number): void;
    pushU16(value: number): void;
    pushStrWithLen(s: string): void;
    copySlice(start: number, end: number): Uint8Array;
    clear(): void;
}
/**
 * Forward-only canonical message builder matching Rust `graven::Builder` byte-for-byte.
 * Start each message with `root(fixedLen)`, populate fields, then emit with
 * `finish()` (bare message), `finishEnvelope(schemaId)`, `seal()`, or `sealEnvelope(schemaId)`.
 */
export declare class Builder {
    private readonly main;
    private sealed;
    private msgStart;
    private sealMode;
    private pendingClose;
    constructor(initialCapacity?: number);
    root(fixedLen: number): TableW;
    _startRpc(): void;
    get _capacity(): number;
    _reset(): void;
    _finishRpcCall(serviceIdVal: bigint, method: number, variant: number): OwnedBytes;
    private flushPending;
    /** Returns the current bare message (throws `TooLarge` if > `i32::MAX`). */
    finish(): Uint8Array;
    /** Wraps the current message in a GRVN v1 envelope and clears the builder. */
    finishEnvelope(id: bigint): Uint8Array;
    /** Seals the current message as `[u32 len][message]` into the pending batch. */
    seal(): void;
    /** Seals the current message as a GRVN v1 envelope into the pending batch. */
    sealEnvelope(id: bigint): void;
    get sealedLen(): number;
    takeFrames(): Uint8Array;
    private closeEnvelope;
}
/**
 * Runs `fn` with a builder reused across calls (used by the generated `toBytes` and by RPC
 * calls), so writing a message does not start from an empty buffer every time. `fn` must
 * return bytes it copied out (`finish*`). A nested call gets a fresh builder.
 */
export declare function withBuilder<R>(fn: (b: Builder) => R): R;
/** Writer for one table's fixed and variable fields. */
export declare class TableW {
    readonly main: GrowBuf;
    private readonly aux;
    private readonly start;
    readonly pendingClose: (() => void)[];
    constructor(main: GrowBuf, aux: GrowBuf | undefined, start: number, pendingClose?: (() => void)[]);
    private fixedTarget;
    private setBit;
    private clearBit;
    patch(off: number, target: number): void;
    set<T>(off: number, presence: Bit, codec: LeCodec<T>, value: T): this;
    setBool(presence: Bit, value: boolean): this;
    setBytes(off: number, bytes: Uint8Array): this;
    setStr(off: number, s: string): this;
    setScalars<T>(off: number, codec: LeCodec<T>, values: Iterable<T>): this;
    setStrs(off: number, values: Iterable<string>): this;
    setBytesList(off: number, values: Iterable<Uint8Array>): this;
    table(off: number, fixedLen: number): TableW;
    union(off: number, tag: number, fixedLen: number): TableW;
    tables(off: number, count: number, elemFixedLen: number): TableListW;
    tablesStreaming(off: number, elemFixedLen: number, ptrSlots: readonly number[]): StreamListW;
    scalarsStreaming<T>(off: number, codec: LeCodec<T>): ScalarListW<T>;
}
/** Fixed-count list-of-tables writer; updates count on every `push()`. */
export declare class TableListW {
    private readonly main;
    private readonly pos;
    private readonly first;
    private readonly efl;
    private readonly cap;
    private readonly pendingClose;
    private count;
    constructor(main: GrowBuf, pos: number, first: number, efl: number, cap: number, pendingClose?: (() => void)[]);
    push(): TableW;
    close(): void;
}
/** Streaming list-of-tables writer; finalizes on `close()`. */
export declare class StreamListW {
    private readonly parent;
    private readonly off;
    private readonly efl;
    private readonly ptrSlots;
    private readonly aux;
    private count;
    private closed;
    constructor(parent: TableW, off: number, efl: number, ptrSlots: readonly number[]);
    push(): TableW;
    close(): void;
}
/**
 * Streaming scalar list writer; updates count on every `push()`. Its elements are appended to
 * the message, so every `push()` must come before any other variable-size field is written.
 */
export declare class ScalarListW<T> {
    private readonly main;
    private readonly pos;
    private readonly codec;
    private count;
    /** Where the next element goes; anything else written there breaks the list. */
    private end;
    constructor(main: GrowBuf, pos: number, codec: LeCodec<T>);
    push(value: T): this;
    close(): void;
}
export declare function utf8Len(s: string): number;
export declare function reqPtr<T>(val: T | undefined, field: string): T;
export declare function optVStrs(s: StrList | undefined): VStrList | undefined;
export declare function reqVStrs(s: StrList | undefined, field: string): VStrList;
/** Typed fixed-count list writer returning typed element writers `W`. */
export declare class ListW<W> {
    private readonly inner;
    private readonly wrap;
    constructor(inner: TableListW, wrap: (w: TableW) => W);
    push(): W;
    close(): void;
}
/** Typed streaming list writer returning typed element writers `W`. */
export declare class StreamW<W> {
    private readonly inner;
    private readonly wrap;
    constructor(inner: StreamListW, wrap: (w: TableW) => W);
    push(): W;
    close(): void;
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
export declare function checkFailure(f: ValidationFailure, r: unknown): ValidationFailure;
/** Thrown by `validate()` when a schema validation rule fails. */
export declare class ValidationError extends Error {
    readonly failure: ValidationFailure;
    constructor(failure: ValidationFailure);
}
export declare const FRAME_VERSION = 1;
/** Header every frame starts with. */
export declare const FRAME_HEADER_LEN = 7;
/** Header of a `Call` frame: `FRAME_HEADER_LEN` plus method and service id. */
export declare const CALL_HEADER_LEN = 17;
export declare const FrameKind: {
    readonly Call: 1;
    readonly Ok: 2;
    readonly Err: 3;
    readonly Item: 4;
    readonly End: 5;
    readonly Fault: 6;
};
export type FrameKind = (typeof FrameKind)[keyof typeof FrameKind];
export declare const FaultCode: {
    readonly Unsupported: 1;
    readonly Malformed: 2;
    readonly Invalid: 3;
    readonly DeadlineExceeded: 4;
    readonly Internal: 5;
    readonly Unavailable: 6;
    readonly isKnown: (code: number) => boolean;
    readonly normalize: (code: number) => number;
};
export declare const BROKEN_RULE_SCHEMA_ID: bigint;
export declare const FAULT_SCHEMA_ID: bigint;
/**
 * Header of an RPC frame. `method` and `serviceId` are on the wire only for a `Call`; a reply
 * belongs to the call it answers, and decodes them as `0` / `0n`.
 */
export interface FrameHeader {
    readonly kind: FrameKind;
    readonly variant: number;
    readonly method: number;
    readonly serviceId: bigint;
}
export interface Frame {
    readonly header: FrameHeader;
    /** The format v1 message the frame carries (empty for `End`), not yet verified. */
    readonly payload: Uint8Array;
    /** Total frame byte length: header plus message. */
    readonly totalLen: number;
}
/** Header length of a frame of `kind`. */
export declare function frameHeaderLen(kind: FrameKind): number;
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
/** The header of a reply frame (`Ok`, `Err`, `Item`, `End`, `Fault`). */
export declare function replyHeader(kind: FrameKind, variant?: number): FrameHeader;
/** The header of a `Call` to `method` of service `serviceId`. */
export declare function callHeader(serviceIdVal: bigint, method: number, variant?: number): FrameHeader;
/** Encodes one RPC frame carrying `payload` (a format v1 message). */
export declare function encodeFrame(header: FrameHeader, payload: Uint8Array): Uint8Array;
/** Encodes a 7-byte `End` RPC frame. */
export declare function encodeEnd(): Uint8Array;
/**
 * Decodes one RPC frame at the start of `buf`, following the normative check order of
 * `docs/spec/graven-rpc-v1.md` §4:
 * 1. `buf.length < 7` -> `Incomplete { needed: 7 }`
 * 2. version (`buf[0] >> 4`) `!== 1` -> `UnsupportedFrameVersion`
 * 3. kind (`buf[0] & 0x0f`) not in `1..=6` -> `BadKind`
 * 4. `len > 2^31 - 1`, or `End` with a variant or a message -> `BadFrameHeader`
 * 5. `buf.length < headerLen + len` -> `Incomplete { needed: headerLen + len }`
 * The message is returned as is: verifying it is the caller's job.
 */
export declare function decodeFrame(buf: Uint8Array): Frame;
/** Iterates over concatenated RPC frames in `buf`. */
export declare function frames(buf: Uint8Array): IterableIterator<Frame>;
/** Verifies and decodes a `graven.rpc.Fault` message payload. */
export declare function decodeFault(payload: Uint8Array): FaultData;
/** Encodes a raw `graven.rpc.Fault` message payload (no envelope or frame header). */
export declare function encodeFaultPayload(fault: FaultData): Uint8Array;
/** Encodes a full `Fault` RPC frame. */
export declare function encodeFaultFrame(fault: FaultData): Uint8Array;
/** Error thrown when the server replies with a `Fault` frame. */
export declare class RpcFault extends Error {
    readonly code: number;
    readonly faultMessage?: string;
    readonly broken?: BrokenRule;
    constructor(fault: FaultData);
}
export type RpcProtocolErrorKind = "Frame" | "Payload" | "EmptyReply" | "UnexpectedKind" | "MissingVariant" | "UnexpectedVariant" | "TrailingBytes" | "UnexpectedOneWayReply" | "Truncated" | "TooLarge";
export interface RpcProtocolErrorDetails {
    readonly causeError?: GravenError;
    readonly frameKind?: number;
    readonly expected?: bigint | number;
    readonly found?: bigint | number;
    readonly variant?: number;
    readonly trailing?: number;
}
/** Error thrown when a reply violates the Graven RPC v1 protocol. */
export declare class RpcProtocolError extends Error {
    readonly kind: RpcProtocolErrorKind;
    readonly causeError?: GravenError;
    readonly frameKind?: number;
    readonly expected?: bigint | number;
    readonly found?: bigint | number;
    readonly variant?: number;
    readonly trailing?: number;
    constructor(kind: RpcProtocolErrorKind, details?: RpcProtocolErrorDetails);
}
/** Default `CallOptions.maxFrameBytes`: 16 MiB. */
export declare const DEFAULT_MAX_FRAME_BYTES: number;
export interface CallOptions {
    timeoutMs?: number;
    /**
     * Largest reply frame accepted, in bytes (default `DEFAULT_MAX_FRAME_BYTES`). A larger
     * frame fails with `RpcProtocolError("TooLarge")` as soon as its header announces it,
     * without buffering it. In a stream it limits each frame, not the whole stream.
     */
    maxFrameBytes?: number;
}
export type ReplyStreamInput = Uint8Array | ReadableStream<Uint8Array> | AsyncIterable<Uint8Array>;
/**
 * Bytes backed by a plain `ArrayBuffer`, as `fetch` (`body`), `WebSocket.send` and `Blob`
 * accept them under TypeScript 5.7 and later. With an older TypeScript it is `Uint8Array`.
 */
export type OwnedBytes = ReturnType<Uint8Array["slice"]>;
/**
 * Sends one call and returns its reply. `call` is a complete frame in its own buffer, so it
 * can be passed straight to `fetch`, a WebSocket or a queue.
 */
export type Sender = (call: OwnedBytes, timeoutMs: number | undefined) => Promise<ReplyStreamInput>;
export type RpcResult<T, E> = {
    readonly ok: true;
    readonly value: T;
} | {
    readonly ok: false;
    readonly error: E;
};
export interface RpcCodec<W, V> {
    encode(fn: (w: W) => void): {
        readonly variant: number;
        readonly payload: Uint8Array;
    };
    encodeTo?(b: Builder, fn: (w: W) => void): {
        readonly variant: number;
    };
    verify(variant: number, payload: Uint8Array): V;
}
export declare function rpcVerifyTable<V>(variant: number, payload: Uint8Array, verifyFn: (buf: Uint8Array) => V): V;
export declare function encodeCallFrame<W, V>(codec: RpcCodec<W, V>, serviceIdVal: bigint, method: number, fn: (w: W) => void): OwnedBytes;
export declare function rpcOneWay<IW, IV>(send: Sender, inCodec: RpcCodec<IW, IV>, serviceIdVal: bigint, method: number, fn: (w: IW) => void, opts?: CallOptions): Promise<void>;
export declare function rpcUnary<IW, IV, OW, OV>(send: Sender, inCodec: RpcCodec<IW, IV>, outCodec: RpcCodec<OW, OV>, serviceIdVal: bigint, method: number, defaultTimeoutMs: number | undefined, fn: (w: IW) => void, opts?: CallOptions): Promise<OV>;
export declare function rpcUnaryResult<IW, IV, OW, OV, EW, EV>(send: Sender, inCodec: RpcCodec<IW, IV>, outCodec: RpcCodec<OW, OV>, errCodec: RpcCodec<EW, EV>, serviceIdVal: bigint, method: number, defaultTimeoutMs: number | undefined, fn: (w: IW) => void, opts?: CallOptions): Promise<RpcResult<OV, EV>>;
export declare function rpcStream<IW, IV, OW, OV>(send: Sender, inCodec: RpcCodec<IW, IV>, outCodec: RpcCodec<OW, OV>, serviceIdVal: bigint, method: number, defaultTimeoutMs: number | undefined, fn: (w: IW) => void, opts?: CallOptions): Promise<AsyncIterable<OV>>;
export declare function rpcStreamResult<IW, IV, OW, OV, EW, EV>(send: Sender, inCodec: RpcCodec<IW, IV>, outCodec: RpcCodec<OW, OV>, errCodec: RpcCodec<EW, EV>, serviceIdVal: bigint, method: number, defaultTimeoutMs: number | undefined, fn: (w: IW) => void, opts?: CallOptions): Promise<RpcResult<AsyncIterable<OV>, EV>>;
export {};
