// Decodes RPC frames into the JSON of testvectors/rpc-v1/*.json, as the Rust reference
// does (`rpc_vectors::decode_expect`). Shared by the conformance test and the fuzzer.
import { decodeFault, FrameKind, frames, GravenError } from "./gen/graven.js";

export type RpcExpect = { frames: unknown[] } | { error: string };

function hex(b: Uint8Array): string {
  let out = "";
  for (const x of b) out += x.toString(16).padStart(2, "0");
  return out;
}

export function frameError(e: GravenError): string {
  switch (e.kind) {
    case "Incomplete":
      return `Incomplete(${String(e.needed)})`;
    case "UnsupportedFrameVersion":
      return `UnsupportedFrameVersion(${String(e.found)})`;
    case "BadKind":
      return `BadKind(${String(e.found)})`;
    default:
      return e.kind;
  }
}

/** Every frame in `bytes`, or the first error; throws only on a bug (not a GravenError). */
export function decodeRpcExpect(bytes: Uint8Array): RpcExpect {
  const out: unknown[] = [];
  try {
    for (const f of frames(bytes)) {
      const fr: Record<string, unknown> = { kind: f.header.kind, variant: f.header.variant };
      if (f.header.kind === FrameKind.Call) {
        fr.method = f.header.method;
        fr.service_id = `0x${f.header.serviceId.toString(16).padStart(16, "0")}`;
      }
      fr.message = hex(f.payload);
      if (f.header.kind === FrameKind.Fault) {
        let fault;
        try {
          fault = decodeFault(f.payload);
        } catch (e) {
          if (e instanceof GravenError) return { error: `Fault(${e.kind})` };
          throw e;
        }
        fr.fault_code = fault.code;
        if (fault.message !== undefined) fr.fault_message = fault.message;
        if (fault.broken !== undefined) {
          const { tableName, ...rest } = fault.broken;
          fr.fault_broken = { table_name: tableName, ...rest };
        }
      }
      out.push(fr);
    }
  } catch (e) {
    if (e instanceof GravenError) return { error: frameError(e) };
    throw e;
  }
  return { frames: out };
}
