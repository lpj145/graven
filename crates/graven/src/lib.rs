//! Runtime for **Graven**, a zero-copy binary serialization format.
//!
//! Messages are read in place: accessors read fields straight from the buffer, and
//! strings and byte slices borrow from it. The wire format is specified in
//! `docs/spec/graven-format-v1.md` (format v1, frozen) and pinned by the conformance
//! vectors in `testvectors/v1/`.
//!
//! Most code does not use this crate directly: the `gravenc` compiler turns a `.graven` schema
//! into typed readers, verified views and writers built on top of it. This crate provides:
//!
//! - [`Builder`] and the writers ([`TableW`], [`FixedW`], list writers): build a message
//!   into a pooled buffer ([`BufPool`], [`SimplePool`]);
//! - [`TableRef`] and the list readers: checked, lazy reads, where every access validates
//!   what it touches and returns an [`Error`] instead of misbehaving;
//! - [`VerifyCtx`]: the depth and budget limits of a full `verify` (spec §7), which
//!   generated code runs once before handing out a verified view;
//! - [`envelope`]: the optional envelope for disk and network (magic, version, schema id,
//!   CRC32C), including iteration over concatenated envelopes;
//! - [`rt`]: support items called by generated code.
//!
//! The crate is `no_std` (with `alloc`) when the default `std` feature is disabled.
//!
//! ```
//! use bytes::BytesMut;
//! use graven::envelope::{decode, encode, schema_id};
//!
//! let id = schema_id("bank.Transaction");
//! let mut out = BytesMut::new();
//! encode(id, b"payload bytes", &mut out)?;
//! let env = decode(&out)?;
//! assert_eq!((env.schema_id, env.payload), (id, &b"payload bytes"[..]));
//! # Ok::<(), graven::Error>(())
//! ```
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_op_in_unsafe_fn)]
// The only `unsafe` is `TableRef::str_unchecked` and the support for generated verified
// views in `rt` (`FromVerified`, `VListR`, `VStrList`); each carries its own `#[allow]`.
#![deny(unsafe_code)]
extern crate alloc;

mod crc;
mod error;
pub mod rt;
mod le;
pub mod read;

pub use bytes::Bytes;
pub use crc::crc32c;
pub use error::Error;
pub use le::Le;
pub use read::{Bit, Frames, ScalarList, StrList, TableList, TableRef, frames};
#[cfg(feature = "std")]
pub use pool::SimplePool;
pub use pool::BufPool;
pub use write::{Builder, FixedListW, FixedW, ScalarListW, StreamListW, TableListW, TableW};
pub mod envelope;
pub mod pool;
pub mod write;
pub mod verify;
pub use verify::VerifyCtx;
