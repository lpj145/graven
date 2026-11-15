//! Code generation dispatch: IR + Lock → Rust (`rust`) or JavaScript with TypeScript
//! declarations (`js`).

pub mod rust;
pub mod js;

/// Target language for `gravenc gen` / `Build`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    Rust,
    /// `.js` files with `.d.ts` declarations.
    Js,
}

/// Options controlling RPC code generation (`--side client`, `--side server`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GenOptions {
    pub client: bool,
    pub server: bool,
}

pub use rust::{generate, generate_with_options, generate_with_source, snake};
