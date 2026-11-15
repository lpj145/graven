//! Stdio RPC server used for Rust <-> TypeScript interoperability testing.
//!
//! Wire framing over stdio: `[u32le length][bytes]` per call and reply (0-length reply for one-way).

use std::io::{self, Read, Write};

use graven::rpc::{Fault, FaultCode, fault_frame_bytes};
use graven_codegen_tests::inventory::{
    Checks, CommandVerified, InventoryCall, INVENTORY_SERVICE_ID,
};

struct NoChecks;
impl Checks for NoChecks {}

fn handle_call(call: &[u8]) -> Vec<u8> {
    match InventoryCall::decode_validated(call, &NoChecks) {
        Ok(c) => match c {
            InventoryCall::GetItem(input, reply) => {
                let sku = input.sku().unwrap().to_owned();
                reply.ok(|w| {
                    w.sku(&sku).stock(42);
                })
            }
            InventoryCall::Reserve(input, reply) => {
                let sku = input.sku().unwrap();
                let qty = input.qty();
                if sku == "unknown" {
                    reply.err(|e| {
                        e.invalid_sku().reason("unknown sku");
                    })
                } else if qty > 10 {
                    reply.err(|e| {
                        e.out_of_stock().available(10);
                    })
                } else {
                    reply.ok(|w| {
                        w.id(9001);
                    })
                }
            }
            InventoryCall::Watch(input, mut reply) => {
                let sku = input.sku().unwrap();
                if sku == "fault-mid-stream" {
                    let mut buf = reply.item(|w| {
                        w.seq(1).stock(50);
                    });
                    buf.extend_from_slice(&fault_frame_bytes(
                        INVENTORY_SERVICE_ID,
                        3,
                        &Fault {
                            code: FaultCode::Unavailable,
                            message: Some("replica lost"),
                            broken: None,
                        },
                    ));
                    buf
                } else {
                    let mut buf = reply.item(|w| {
                        w.seq(1).stock(10);
                    });
                    buf.extend_from_slice(&reply.item(|w| {
                        w.seq(2).stock(8);
                    }));
                    buf.extend_from_slice(&reply.end());
                    buf
                }
            }
            InventoryCall::Batch(input, mut reply) => {
                let count = input.count();
                if count == 99 {
                    reply.err(|e| {
                        e.code(503);
                    })
                } else {
                    let mut buf = Vec::new();
                    for i in 0..count {
                        buf.extend_from_slice(&reply.item(|w| {
                            w.index(i);
                        }));
                    }
                    buf.extend_from_slice(&reply.end());
                    buf
                }
            }
            InventoryCall::Audit(_) => Vec::new(),
            InventoryCall::Dispatch(input, reply) => match input {
                CommandVerified::Add(cmd) => {
                    let delta = cmd.delta();
                    reply.ok(|o| {
                        o.added().new_stock((100 + delta) as u32);
                    })
                }
                CommandVerified::Clear(_) => reply.ok(|o| {
                    o.cleared().removed(100);
                }),
                CommandVerified::Unknown(_) => reply.err(|e| {
                    e.available(0);
                }),
            },
        },
        Err(rej) => rej.to_frame().unwrap_or_default(),
    }
}

fn main() {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut len_buf = [0u8; 4];

    while match stdin.read_exact(&mut len_buf) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => false,
        Err(e) => panic!("stdin read len failed: {e}"),
    } {
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut call = vec![0u8; len];
        stdin.read_exact(&mut call).expect("stdin read call body failed");
        let reply = handle_call(&call);
        let rlen = u32::try_from(reply.len()).expect("reply fits u32").to_le_bytes();
        stdout.write_all(&rlen).expect("stdout write len failed");
        stdout.write_all(&reply).expect("stdout write reply failed");
        stdout.flush().expect("stdout flush failed");
    }
}
