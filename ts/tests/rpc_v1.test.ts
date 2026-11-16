import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  FaultCode,
  FrameKind,
  GravenError,
  RpcFault,
  RpcProtocolError,
  decodeFault,
  decodeFrame,
  encodeEnd,
  encodeFaultFrame,
  encodeFaultPayload,
  encodeFrame,
  frames,
  schemaId,
} from "./gen/graven.js";
import {
  Event,
  EventW,
  INVENTORY_SERVICE_ID,
  InventoryClient,
  Item,
  ItemW,
} from "./gen/inventory.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RPC_VECTORS_DIR = path.resolve(HERE, "../../../testvectors/rpc-v1");

interface ExpectedRpcFrame {
  kind: number;
  method: number;
  variant: number;
  service_id: string;
  schema_id: string;
  payload_len: number;
  fault_code?: number;
  fault_message?: string;
  fault_broken?: {
    table_name: string;
    field?: string;
    index?: number;
    rule: string;
    min?: string;
    max?: string;
    reason?: string;
  };
}

interface RpcVectorSpec {
  frames?: ExpectedRpcFrame[];
  error?: string;
}

function hexU64(v: bigint): string {
  return `0x${v.toString(16).padStart(16, "0")}`;
}

function formatFrameErrorString(e: GravenError): string {
  switch (e.kind) {
    case "Incomplete":
      return `Incomplete(${String(e.needed)})`;
    case "UnsupportedFrameVersion":
      return `UnsupportedFrameVersion(${String(e.found)})`;
    case "BadKind":
      return `BadKind(${String(e.found)})`;
    case "BadFrameHeader":
      return "BadFrameHeader";
    default:
      return `Envelope(${e.kind})`;
  }
}

describe("testvectors/rpc-v1 conformance", () => {
  const jsonFiles = fs
    .readdirSync(RPC_VECTORS_DIR)
    .filter((f) => f.endsWith(".json"))
    .sort();

  it("finds all 18 rpc-v1 vector pairs (36 files)", () => {
    assert.equal(jsonFiles.length, 18);
  });

  for (const file of jsonFiles) {
    const name = file.slice(0, -5);
    it(name, () => {
      const spec = JSON.parse(fs.readFileSync(path.join(RPC_VECTORS_DIR, file), "utf8")) as RpcVectorSpec;
      const bin = fs.readFileSync(path.join(RPC_VECTORS_DIR, `${name}.bin`));
      if (spec.error !== undefined) {
        assert.throws(
          () => decodeFrame(bin),
          (err: unknown) => {
            if (!(err instanceof GravenError)) return false;
            return formatFrameErrorString(err) === spec.error;
          },
        );
      } else {
        const expected = spec.frames ?? [];
        const decoded = [...frames(bin)];
        assert.equal(decoded.length, expected.length);
        for (let i = 0; i < decoded.length; i++) {
          const f = decoded[i]!;
          const exp = expected[i]!;
          assert.equal(f.header.kind, exp.kind);
          assert.equal(f.header.method, exp.method);
          assert.equal(f.header.variant, exp.variant);
          assert.equal(hexU64(f.header.serviceId), exp.service_id);
          assert.equal(hexU64(f.schemaId), exp.schema_id);
          assert.equal(f.payload.length, exp.payload_len);
          if (f.header.kind === FrameKind.Fault) {
            const fault = decodeFault(f.payload);
            assert.equal(fault.code, exp.fault_code);
            assert.equal(fault.message, exp.fault_message);
            if (exp.fault_broken !== undefined) {
              const { table_name, ...rest } = exp.fault_broken;
              assert.deepEqual(fault.broken, { tableName: table_name, ...rest });
            }
          }
        }
      }
    });
  }
});

describe("rpc fault encode/decode & unknown codes", () => {
  it("round-trips Fault with BrokenRule and optional fields", () => {
    const sid = schemaId("test.svc.v1.Svc");
    const frameBytes = encodeFaultFrame(sid, 4, {
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
    assert.equal(f.header.kind, FrameKind.Fault);
    assert.equal(f.header.method, 4);
    assert.equal(f.header.variant, 0);
    assert.equal(f.header.serviceId, sid);
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
    // Without codes the first-release layout is kept, so those bytes do not change.
    assert.equal(long.length > short.length, true);
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
    const p1 = EventW.toBytes((w) => w.seq(1n).stock(5));
    const p2 = EventW.toBytes((w) => w.seq(2n).stock(4));
    // Extract raw payload from envelopes cleanly by decoding frame or stripping envelope;
    // even simpler: use b.finish / Item encoding via encodeFrame with Event payload.
    const env1 = decodeFrame(
      encodeFrame({ kind: FrameKind.Item, method: 3, variant: 0, serviceId: INVENTORY_SERVICE_ID }, Event.SCHEMA_ID, Event.fromBytes(p1)._ref.buf),
    );
    const f1 = encodeFrame(env1.header, Event.SCHEMA_ID, env1.payload);
    const f2 = encodeFrame(
      { kind: FrameKind.Item, method: 3, variant: 0, serviceId: INVENTORY_SERVICE_ID },
      Event.SCHEMA_ID,
      Event.fromBytes(p2)._ref.buf,
    );
    const end = encodeEnd(INVENTORY_SERVICE_ID, 3);
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
    const itemBytes = ItemW.toBytes((w) => w.sku("abc").stock(3));
    const itemPayload = Item.fromBytes(itemBytes)._ref.buf;
    const reply = encodeFrame(
      { kind: FrameKind.Ok, method: 1, variant: 0, serviceId: INVENTORY_SERVICE_ID },
      Item.SCHEMA_ID,
      itemPayload,
    );
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
      encodeFaultFrame(INVENTORY_SERVICE_ID, 1, { code: FaultCode.Unavailable, message: "busy" }),
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

    const truncClient = new InventoryClient(async () => new Uint8Array([1, 2, 3, 4]));
    await assert.rejects(
      truncClient.watch((w) => w.sku("a")),
      (err: unknown) => err instanceof RpcProtocolError && err.kind === "Truncated",
    );

    const mismatchClient = new InventoryClient(async () =>
      encodeEnd(INVENTORY_SERVICE_ID + 1n, 3),
    );
    await assert.rejects(
      mismatchClient.watch((w) => w.sku("a")),
      (err: unknown) => err instanceof RpcProtocolError && err.kind === "ServiceMismatch",
    );

    const badOneWay = new InventoryClient(async () => new Uint8Array([1]));
    await assert.rejects(
      badOneWay.audit((w) => w.actor("op").note("hi")),
      (err: unknown) => err instanceof RpcProtocolError && err.kind === "UnexpectedOneWayReply",
    );
  });
});
