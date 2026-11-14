//! `.graven` compiler (`gravenc`): AST, lexer and parser of the schema language, semantic
//! check and resolved IR with layout, compatibility checks, Rust code generation, and the
//! data tools behind `gravenc inspect`, `scan`, `diff`, `to-json` and `from-json`.

pub mod ast;
pub mod build;
pub mod check;
pub mod codegen;
pub mod compat;
pub mod diff;
pub mod dynamic;
pub mod ir;
pub mod json;
pub mod lock;
pub mod names;
pub mod parse;

pub use ast::*;
pub use check::check;
pub use ir::*;
pub use parse::parse;
