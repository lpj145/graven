import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { decodeEnvelope, GravenError } from "./gen/graven.js";
import { type JsonVal, readCase } from "./conformance_json.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const VECTORS_DIR = path.resolve(HERE, "../../../testvectors/v1");

describe("testvectors/v1 conformance (generated conformance.ts)", () => {
  const jsonFiles = fs
    .readdirSync(VECTORS_DIR)
    .filter((f) => f.endsWith(".json"))
    .sort();

  it("finds all 42 test vectors", () => {
    assert.equal(jsonFiles.length, 42);
  });

  for (const file of jsonFiles) {
    const name = file.slice(0, -5);
    it(name, () => {
      const spec = JSON.parse(fs.readFileSync(path.join(VECTORS_DIR, file), "utf8")) as {
        schema: string;
        enveloped: boolean;
        expect: { value?: JsonVal; error?: string };
      };
      const bin = fs.readFileSync(path.join(VECTORS_DIR, `${name}.bin`));
      if (spec.expect.error !== undefined) {
        assert.throws(
          () => readCase(spec.schema, spec.enveloped, bin),
          (err: unknown) => err instanceof GravenError && err.kind === spec.expect.error,
        );
      } else {
        const actual = readCase(spec.schema, spec.enveloped, bin);
        assert.deepEqual(actual, spec.expect.value);
      }
    });
  }
});

describe("envelope check order (format v1 §8.4)", () => {
  it("a payload_len past the input is Incomplete, however large", () => {
    for (const len of [0x7fffffff, 0x80000000, 0xffffffff]) {
      const buf = new Uint8Array(24);
      buf.set([0x47, 0x52, 0x56, 0x4e, 1]);
      new DataView(buf.buffer).setUint32(16, len, true);
      assert.throws(
        () => decodeEnvelope(buf),
        (err: unknown) => err instanceof GravenError && err.kind === "Incomplete" && err.needed === 24 + len,
      );
    }
  });
});
