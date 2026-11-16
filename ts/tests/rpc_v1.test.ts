import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  Builder,
  FaultCode,
  FrameKind,
  RpcFault,
  RpcProtocolError,
  decodeFault,
  decodeFrame,
  encodeEnd,
  encodeFaultFrame,
  encodeFaultPayload,
  encodeFrame,
  replyHeader,
} from "./gen/graven.js";
import { decodeRpcExpect, type RpcExpect } from "./rpc_json.js";
import {
  EventW,
  InventoryClient,
  ItemW,
} from "./gen/inventory.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RPC_VECTORS_DIR = path.resolve(HERE, "../../../testvectors/rpc-v1");

describe("testvectors/rpc-v1 conformance", () => {
  const jsonFiles = fs
    .readdirSync(RPC_VECTORS_DIR)
    .filter((f) => f.endsWith(".json"))
    .sort();

  it("finds all 23 rpc-v1 vector pairs (46 files)", () => {
    assert.equal(jsonFiles.length, 23);
  });

  for (const file of jsonFiles) {
    const name = file.slice(0, -5);
    it(name, () => {
      const spec = JSON.parse(fs.readFileSync(path.join(RPC_VECTORS_DIR, file), "utf8")) as RpcExpect;
      const bin = fs.readFileSync(path.join(RPC_VECTORS_DIR, `${name}.bin`));
      assert.deepEqual(decodeRpcExpect(bin), spec);
    });
  }
});

describe("rpc fault encode/decode & unknown codes", () => {
  it("round-trips Fault with BrokenRule and optional fields", () => {
    const frameBytes = encodeFaultFrame({
      code: FaultCode.Invalid,
      message: "bad field",
      broken: {
        tableName: "ReserveReq",
        field: "qty",
        index: 2,
        rule: "range",
      },
    });
    const f = decodeFrame(frameBytes);
    assert.deepEqual(f.header, replyHeader(FrameKind.Fault));
    const decoded = decodeFault(f.payload);
    assert.equal(decoded.code, FaultCode.Invalid);
    assert.equal(decoded.message, "bad field");
    assert.deepEqual(decoded.broken, {
      tableName: "ReserveReq",
      field: "qty",
      index: 2,
      rule: "range",
    });
  });

  it("round-trips the codes of a BrokenRule (bounds and check reason)", () => {
    const base = { tableName: "Signup", field: "email", rule: "check(email)" };
    const short = encodeFaultPayload({ code: FaultCode.Invalid, broken: base });
    const withCodes = { ...base, reason: "missing_at" };
    const long = encodeFaultPayload({ code: FaultCode.Invalid, broken: withCodes });
    assert.deepEqual(decodeFault(short).broken, base);
    assert.deepEqual(decodeFault(long).broken, withCodes);
    const bounds = { tableName: "Signup", field: "age", rule: "range", min: "18", max: "120" };
    assert.deepEqual(decodeFault(encodeFaultPayload({ code: FaultCode.Invalid, broken: bounds })).broken, bounds);
  });

  it("preserves unknown future FaultCode and normalizes it to Internal", () => {
    const payload = encodeFaultPayload({ code: 99, message: "future" });
    const decoded = decodeFault(payload);
    assert.equal(decoded.code, 99);
    assert.equal(FaultCode.isKnown(decoded.code), false);
    assert.equal(FaultCode.normalize(decoded.code), FaultCode.Internal);
  });
});

describe("chunked stream & protocol checks on InventoryClient", () => {
  async function* oneByteChunks(buf: Uint8Array): AsyncIterable<Uint8Array> {
    for (let i = 0; i < buf.length; i++) {
      yield buf.subarray(i, i + 1);
    }
  }

  it("decodes streamed frames across 1-byte chunks and ReadableStream", async () => {
    const ev = (seq: bigint, stock: number) => {
      const b = new Builder();
      EventW.root(b).seq(seq).stock(stock);
      return encodeFrame(replyHeader(FrameKind.Item), b.finish());
    };
    const f1 = ev(1n, 5);
    const f2 = ev(2n, 4);
    const end = encodeEnd();
    const all = new Uint8Array(f1.length + f2.length + end.length);
    all.set(f1, 0);
    all.set(f2, f1.length);
    all.set(end, f1.length + f2.length);

    let seenTimeout: number | undefined;
    const clientChunks = new InventoryClient(async (_call, timeoutMs) => {
      seenTimeout = timeoutMs;
      return oneByteChunks(all);
    });
    const stream = await clientChunks.watch((w) => w.sku("coffee"));
    assert.equal(seenTimeout, 10000);
    const seqs: bigint[] = [];
    for await (const ev of stream) {
      seqs.push(ev.seq());
    }
    assert.deepEqual(seqs, [1n, 2n]);

    const clientWebStream = new InventoryClient(async () => {
      return new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(all.subarray(0, 7));
          controller.enqueue(all.subarray(7));
          controller.close();
        },
      });
    });
    const stream2 = await clientWebStream.watch((w) => w.sku("coffee"), { timeoutMs: 250 });
    let count = 0;
    for await (const _ev of stream2) count++;
    assert.equal(count, 2);
  });

  it("reports declared default timeout vs override", async () => {
    const observed: Array<number | undefined> = [];
    const ib = new Builder();
    ItemW.root(ib).sku("abc").stock(3);
    const reply = encodeFrame(replyHeader(FrameKind.Ok), ib.finish());
    const client = new InventoryClient(async (_call, timeoutMs) => {
      observed.push(timeoutMs);
      return reply;
    });
    const a = await client.get_item((w) => w.sku("abc"));
    assert.equal(a.sku(), "abc");
    const b = await client.get_item((w) => w.sku("abc"), { timeoutMs: 777 });
    assert.equal(b.stock(), 3);
    assert.deepEqual(observed, [2500, 777]);
  });

  it("throws RpcFault and RpcProtocolError variants on bad replies", async () => {
    const faultClient = new InventoryClient(async () =>
      encodeFaultFrame({ code: FaultCode.Unavailable, message: "busy" }),
    );
    await assert.rejects(
      faultClient.get_item((w) => w.sku("a")),
      (err: unknown) => err instanceof RpcFault && err.code === FaultCode.Unavailable && err.message === "busy",
    );

    const emptyClient = new InventoryClient(async () => new Uint8Array(0));
    await assert.rejects(
      emptyClient.get_item((w) => w.sku("a")),
      (err: unknown) => err instanceof RpcProtocolError && err.kind === "EmptyReply",
    );

    const truncClient = new InventoryClient(async () => new Uint8Array([0x14, 0, 0, 9]));
    await assert.rejects(
      truncClient.watch((w) => w.sku("a")),
      (err: unknown) => err instanceof RpcProtocolError && err.kind === "Truncated",
    );

    // A reply in an older frame layout (first byte 0x01) is refused by its version.
    const oldClient = new InventoryClient(async () => new Uint8Array([1, 2, 1, 0, 0, 0, 0, 0]));
    await assert.rejects(
      oldClient.get_item((w) => w.sku("a")),
      (err: unknown) =>
        err instanceof RpcProtocolError && err.kind === "Frame" && err.causeError?.kind === "UnsupportedFrameVersion",
    );

    const badOneWay = new InventoryClient(async () => new Uint8Array([1]));
    await assert.rejects(
      badOneWay.audit((w) => w.actor("op").note("hi")),
      (err: unknown) => err instanceof RpcProtocolError && err.kind === "UnexpectedOneWayReply",
    );
  });
});

describe("reply size limit (CallOptions.maxFrameBytes)", () => {
  const bigItem = () => {
    const b = new Builder();
    ItemW.root(b).sku("x".repeat(2000)).stock(1);
    return encodeFrame(replyHeader(FrameKind.Ok), b.finish());
  };
  const isTooLarge = (e: unknown) => e instanceof RpcProtocolError && e.kind === "TooLarge";

  it("rejects a unary reply larger than the limit, whole or in chunks", async () => {
    const reply = bigItem();
    const whole = new InventoryClient(async () => reply);
    await assert.rejects(whole.get_item((w) => w.sku("x"), { maxFrameBytes: 1024 }), isTooLarge);
    async function* chunked() {
      for (let i = 0; i < reply.length; i += 100) yield reply.slice(i, i + 100);
    }
    const parts = new InventoryClient(async () => chunked());
    await assert.rejects(parts.get_item((w) => w.sku("x"), { maxFrameBytes: 1024 }), isTooLarge);
    // Within the limit it reads normally.
    const ok = await whole.get_item((w) => w.sku("x"), { maxFrameBytes: 4096 });
    assert.equal(ok.stock(), 1);
  });

  it("stops at a header that declares a frame past the limit, without waiting for the bytes", async () => {
    // A frame header announcing a ~1 GiB message, then a source that never ends.
    const head = bigItem().slice(0, 7);
    new DataView(head.buffer).setUint32(3, 1 << 30, true);
    let pulled = 0;
    const endless: AsyncIterable<Uint8Array> = {
      [Symbol.asyncIterator]() {
        return {
          async next() {
            pulled++;
            return { done: false, value: pulled === 1 ? head : new Uint8Array(1024) };
          },
        };
      },
    };
    const client = new InventoryClient(async () => endless);
    await assert.rejects(client.watch((w) => w.sku("x"), { maxFrameBytes: 1 << 20 }), isTooLarge);
    assert.equal(pulled < 10, true);
  });

  it("limits each frame of a stream, not the whole stream", async () => {
    const ev = (n: bigint) => {
      const b = new Builder();
      EventW.root(b).seq(n).stock(1);
      return encodeFrame(replyHeader(FrameKind.Item), b.finish());
    };
    async function* many() {
      for (let i = 0n; i < 200n; i++) yield ev(i);
      yield encodeEnd();
    }
    const client = new InventoryClient(async () => many());
    let n = 0;
    for await (const _ of await client.watch((w) => w.sku("x"), { maxFrameBytes: 256 })) n++;
    assert.equal(n, 200);
  });
});
