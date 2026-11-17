// Mutation fuzzer for the JavaScript runtime and generated code (scripts/fuzz-js.sh).
//
// Starting from the conformance vectors and valid replies, it mutates bytes and checks:
//   1. format v1: reading with the generated code gives exactly what the Rust reference gives
//      (`conformance-oracle`): the same value, or the same error;
//   2. RPC frames v1: decoding frames and faults gives exactly what Rust gives;
//   3. the generated client, fed mutated replies (whole or in chunks), always settles, throws
//      only `RpcFault` or `RpcProtocolError`, and any value it returns reads in full.
// Any other exception, a hang or a difference is a failure: the input is saved in
// ts/fuzz-crashes/ and the run exits with status 1.
//
// Usage: node ts/dist/tests/fuzz.js [--seconds N] [--seed N]

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import {
  Builder,
  encodeEnd,
  encodeFaultFrame,
  encodeFrame,
  FaultCode,
  FrameKind,
  GravenError,
  replyHeader,
  RpcFault,
  RpcProtocolError,
  type ReplyStreamInput,
} from "./gen/graven.js";
import { readCase } from "./conformance_json.js";
import { decodeRpcExpect } from "./rpc_json.js";
import {
  BatchErrW,
  BatchItemW,
  EventW,
  InventoryClient,
  InvalidSkuW,
  ItemW,
  OutOfStockW,
  ReceiptW,
} from "./gen/inventory.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../../..");
const ORACLE = process.env.GRAVEN_ORACLE ?? path.join(REPO, "target/debug/conformance-oracle");
const CRASH_DIR = path.join(REPO, "ts/fuzz-crashes");

// ---- arguments and random numbers ----

function arg(name: string, fallback: number): number {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] !== undefined ? Number(process.argv[i + 1]) : fallback;
}
const SECONDS = arg("--seconds", 60);
const SEED = arg("--seed", Date.now() % 0xffffffff);

let rngState = SEED >>> 0 || 1;
function rand(n: number): number {
  // xorshift32
  rngState ^= rngState << 13;
  rngState >>>= 0;
  rngState ^= rngState >>> 17;
  rngState ^= rngState << 5;
  rngState >>>= 0;
  return rngState % n;
}

// ---- mutation ----

const INTERESTING_U8 = [0x00, 0x01, 0x02, 0x7f, 0x80, 0xfe, 0xff];
const INTERESTING_U32 = [0, 1, 2, 3, 4, 8, 16, 0x7f, 0x80, 0xff, 0x100, 0x7fff, 0xffff, 0x7fffffff, 0x80000000, 0xffffffff];

function mutate(input: Uint8Array, pool: readonly Uint8Array[]): Uint8Array {
  let b = Array.from(input);
  const ops = 1 + rand(4);
  for (let k = 0; k < ops; k++) {
    const pos = b.length === 0 ? 0 : rand(b.length);
    switch (rand(9)) {
      case 0: // flip a bit
        if (b.length > 0) b[pos] = b[pos]! ^ (1 << rand(8));
        break;
      case 1: // random byte
        if (b.length > 0) b[pos] = rand(256);
        break;
      case 2: // interesting byte
        if (b.length > 0) b[pos] = INTERESTING_U8[rand(INTERESTING_U8.length)]!;
        break;
      case 3: {
        // interesting little-endian u32 (offsets, lengths)
        let v = INTERESTING_U32[rand(INTERESTING_U32.length)]!;
        if (rand(3) === 0) v = (b.length + rand(5) - 2) >>> 0;
        for (let i = 0; i < 4 && pos + i < b.length; i++) b[pos + i] = (v >>> (8 * i)) & 0xff;
        break;
      }
      case 4: // small add to a byte
        if (b.length > 0) b[pos] = (b[pos]! + rand(9) - 4) & 0xff;
        break;
      case 5: // truncate
        b = b.slice(0, pos);
        break;
      case 6: // insert bytes
        b.splice(pos, 0, ...Array.from({ length: 1 + rand(4) }, () => rand(256)));
        break;
      case 7: // delete bytes
        b.splice(pos, 1 + rand(4));
        break;
      case 8: {
        // splice in a piece of another input
        const other = pool[rand(pool.length)]!;
        if (other.length > 0) {
          const from = rand(other.length);
          const piece = Array.from(other.subarray(from, from + 1 + rand(16)));
          b.splice(pos, rand(2) === 0 ? 0 : piece.length, ...piece);
        }
        break;
      }
    }
  }
  return Uint8Array.from(b);
}

// ---- failures ----

let failures = 0;
function fail(kind: string, detail: string, input: Uint8Array): void {
  failures++;
  fs.mkdirSync(CRASH_DIR, { recursive: true });
  const file = path.join(CRASH_DIR, `${kind}-${SEED}-${failures}.bin`);
  fs.writeFileSync(file, input);
  console.error(`FAIL ${kind}: ${detail}\n  input saved to ${file}`);
}

function hex(b: Uint8Array): string {
  let out = "";
  for (const x of b) out += x.toString(16).padStart(2, "0");
  return out;
}

// ---- seeds ----

interface FormatSeed {
  schema: string;
  enveloped: boolean;
  bytes: Uint8Array;
}

const v1Dir = path.join(REPO, "testvectors/v1");
const formatSeeds: FormatSeed[] = fs
  .readdirSync(v1Dir)
  .filter((f) => f.endsWith(".json"))
  .sort()
  .map((f) => {
    const spec = JSON.parse(fs.readFileSync(path.join(v1Dir, f), "utf8")) as { schema: string; enveloped: boolean };
    return { schema: spec.schema, enveloped: spec.enveloped, bytes: fs.readFileSync(path.join(v1Dir, f.replace(/\.json$/, ".bin"))) };
  });

const rpcDir = path.join(REPO, "testvectors/rpc-v1");
const rpcSeeds: Uint8Array[] = fs
  .readdirSync(rpcDir)
  .filter((f) => f.endsWith(".bin"))
  .sort()
  .map((f) => new Uint8Array(fs.readFileSync(path.join(rpcDir, f))));

function payload<W>(root: (b: Builder) => W, fill: (w: W) => void): Uint8Array {
  const b = new Builder();
  fill(root(b));
  return b.finish();
}
function frame(kind: FrameKind, variant: number, body: Uint8Array): Uint8Array {
  return encodeFrame(replyHeader(kind, variant), body);
}
function concat(...parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}
const end = (): Uint8Array => encodeEnd();
const fault = (): Uint8Array =>
  encodeFaultFrame({
    code: FaultCode.Invalid,
    broken: { tableName: "ReserveReq", field: "qty", rule: "range", min: "1", max: "1000" },
  });

type Method = "get_item" | "reserve" | "watch" | "batch" | "audit";
const item = payload(ItemW.root, (w) => w.sku("espresso").stock(7));
const event = (seq: bigint) => payload(EventW.root, (w) => w.seq(seq).stock(3));
const batchItem = (i: number) => payload(BatchItemW.root, (w) => w.index(i));
const replySeeds: Record<Method, Uint8Array[]> = {
  get_item: [frame(FrameKind.Ok, 0, item), fault()],
  reserve: [
    frame(FrameKind.Ok, 0, payload(ReceiptW.root, (w) => w.id(42n))),
    frame(FrameKind.Err, 1, payload(OutOfStockW.root, (w) => w.available(2))),
    frame(FrameKind.Err, 2, payload(InvalidSkuW.root, (w) => w.reason("bad"))),
    fault(),
  ],
  watch: [
    concat(frame(FrameKind.Item, 0, event(1n)), frame(FrameKind.Item, 0, event(2n)), end()),
    concat(frame(FrameKind.Item, 0, event(1n)), fault()),
  ],
  batch: [
    concat(frame(FrameKind.Item, 0, batchItem(0)), frame(FrameKind.Item, 0, batchItem(1)), end()),
    frame(FrameKind.Err, 0, payload(BatchErrW.root, (w) => w.code(9))),
  ],
  audit: [new Uint8Array(0), end()],
};
const allSeeds: Uint8Array[] = [...formatSeeds.map((s) => s.bytes), ...rpcSeeds, ...Object.values(replySeeds).flat()];

// ---- 1 and 2: differential against the Rust reference ----

type Outcome = { value: unknown } | { error: string } | { frames: unknown[] };

function jsFormat(seed: FormatSeed, bytes: Uint8Array): Outcome | Error {
  try {
    return { value: readCase(seed.schema, seed.enveloped, bytes) };
  } catch (e) {
    if (e instanceof GravenError) return { error: e.kind };
    return e instanceof Error ? e : new Error(String(e));
  }
}

function jsRpc(bytes: Uint8Array): Outcome | Error {
  try {
    return decodeRpcExpect(bytes);
  } catch (e) {
    return e instanceof Error ? e : new Error(String(e));
  }
}

interface Pending {
  line: string;
  bytes: Uint8Array;
  kind: "format" | "rpc";
  js: Outcome;
}

// Errors from the envelope checks, whose order is normative (format v1 §8.4).
const ENVELOPE_ERRORS = new Set(["Incomplete", "BadMagic", "UnsupportedVersion", "BadHeader", "TooLarge", "ChecksumMismatch", "SchemaMismatch"]);

/**
 * Format v1 §7: when a message has more than one defect, a reader MAY return any applicable
 * error, so two errors match unless one comes from the envelope (§8.4, normative order).
 * Values must be equal, and RPC frame errors follow a normative order too.
 */
function sameOutcome(p: Pending, rust: Outcome): boolean {
  if (isDeepStrictEqual(p.js, rust)) return true;
  if (p.kind === "format" && "error" in p.js && "error" in rust) {
    return !ENVELOPE_ERRORS.has(p.js.error) && !ENVELOPE_ERRORS.has(rust.error);
  }
  return false;
}

function checkBatch(batch: Pending[]): void {
  if (batch.length === 0) return;
  const res = spawnSync(ORACLE, { input: batch.map((p) => p.line).join("\n") + "\n", maxBuffer: 1 << 30 });
  if (res.status !== 0) {
    throw new Error(`conformance-oracle failed: ${new TextDecoder().decode(res.stderr)}`);
  }
  const lines = new TextDecoder().decode(res.stdout).trimEnd().split("\n");
  batch.forEach((p, i) => {
    const rust = JSON.parse(lines[i]!) as Outcome;
    if (!sameOutcome(p, rust)) {
      fail(`${p.kind}-diff`, `JS ${JSON.stringify(p.js).slice(0, 300)}\n  Rust ${JSON.stringify(rust).slice(0, 300)}`, p.bytes);
    }
  });
}

// ---- 3: the generated client on mutated replies ----

function deepRead(v: unknown, depth = 0): void {
  if (v === null || typeof v !== "object" || depth > 64) return;
  if (v instanceof Uint8Array) return;
  const name = (v as object).constructor?.name ?? "";
  if (name.endsWith("Verified")) {
    const proto = Object.getPrototypeOf(v) as Record<string, unknown>;
    for (const key of Object.getOwnPropertyNames(proto)) {
      const fn = proto[key];
      if (key === "constructor" || key.startsWith("_") || key.startsWith("validate") || key === "reader") continue;
      if (typeof fn === "function" && fn.length === 0) deepRead((fn as () => unknown).call(v), depth + 1);
    }
    return;
  }
  if (Symbol.iterator in (v as object)) {
    for (const x of v as Iterable<unknown>) deepRead(x, depth + 1);
    return;
  }
  for (const x of Object.values(v as object)) deepRead(x, depth + 1);
}

async function* chunks(b: Uint8Array): AsyncIterable<Uint8Array> {
  let o = 0;
  while (o < b.length) {
    const n = 1 + rand(16);
    yield b.slice(o, o + n);
    o += n;
  }
}

async function callWith(method: Method, reply: Uint8Array): Promise<void> {
  const deliver = (): ReplyStreamInput => (rand(2) === 0 ? reply : chunks(reply));
  const client = new InventoryClient(async () => deliver());
  switch (method) {
    case "get_item":
      deepRead(await client.get_item((w) => w.sku("espresso")));
      return;
    case "reserve":
      deepRead(await client.reserve((w) => w.sku("espresso").qty(1)));
      return;
    case "watch": {
      const items = await client.watch((w) => w.sku("espresso"));
      if (rand(2) === 0) {
        for await (const ev of items) deepRead(ev);
      } else {
        for await (const batch of items.batches()) for (const ev of batch) deepRead(ev);
      }
      return;
    }
    case "batch": {
      const r = await client.batch((w) => w.count(2));
      if (r.ok) {
        for await (const it of r.value) deepRead(it);
      } else {
        deepRead(r.error);
      }
      return;
    }
    case "audit":
      await client.audit((w) => w.actor("a").note("n"));
      return;
  }
}

async function checkClient(method: Method, reply: Uint8Array): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const hang = new Promise<"hang">((resolve) => {
    timer = setTimeout(() => resolve("hang"), 2000);
  });
  try {
    const r = await Promise.race([callWith(method, reply).then(() => "ok" as const), hang]);
    if (r === "hang") fail(`client-hang-${method}`, "the call did not settle in 2 s", reply);
  } catch (e) {
    if (!(e instanceof RpcFault) && !(e instanceof RpcProtocolError)) {
      fail(`client-${method}`, `unexpected ${e instanceof Error ? `${e.name}: ${e.message}` : String(e)}`, reply);
    }
  } finally {
    clearTimeout(timer);
  }
}

// ---- main loop ----

async function main(): Promise<void> {
  if (!fs.existsSync(ORACLE)) {
    throw new Error(`missing ${ORACLE}: cargo build -p graven-conformance --bin conformance-oracle`);
  }
  console.log(`fuzzing for ${SECONDS} s, seed ${SEED}`);
  const deadline = Date.now() + SECONDS * 1000;
  const counts = { format: 0, rpc: 0, client: 0 };
  const methods = Object.keys(replySeeds) as Method[];
  while (Date.now() < deadline && failures < 20) {
    const batch: Pending[] = [];
    for (let i = 0; i < 2000; i++) {
      if (rand(2) === 0) {
        const seed = formatSeeds[rand(formatSeeds.length)]!;
        const bytes = mutate(seed.bytes, allSeeds);
        const js = jsFormat(seed, bytes);
        if (js instanceof Error) {
          fail("format-throw", `${js.name}: ${js.message}`, bytes);
          continue;
        }
        batch.push({ line: `format ${seed.schema} ${seed.enveloped ? 1 : 0} ${hex(bytes)}`, bytes, kind: "format", js });
        counts.format++;
      } else {
        const bytes = mutate(rpcSeeds[rand(rpcSeeds.length)]!, allSeeds);
        const js = jsRpc(bytes);
        if (js instanceof Error) {
          fail("rpc-throw", `${js.name}: ${js.message}`, bytes);
          continue;
        }
        batch.push({ line: `rpc ${hex(bytes)}`, bytes, kind: "rpc", js });
        counts.rpc++;
      }
    }
    checkBatch(batch);
    for (let i = 0; i < 300; i++) {
      const method = methods[rand(methods.length)]!;
      const seeds = replySeeds[method];
      await checkClient(method, mutate(seeds[rand(seeds.length)]!, allSeeds));
      counts.client++;
    }
  }
  console.log(`format ${counts.format}, rpc ${counts.rpc}, client ${counts.client} inputs; ${failures} failure(s)`);
  if (failures > 0) process.exit(1);
}

await main();
