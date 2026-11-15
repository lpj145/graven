//! Generates `edge` (`schemas/edge.graven`, edge cases) and `names` (`schemas/names.graven`,
//! tricky names) and `defaults` (`schemas/defaults.graven`, defaults at the extremes of every
//! type) in `OUT_DIR` with the `graven-compiler` build API, reading the committed locks
//! (`<schema>.lock`); never writes them.

use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    graven_compiler::build::Build::new()
        .schema(dir.join("schemas/edge.graven"))
        .schema(dir.join("schemas/names.graven"))
        .schema(dir.join("schemas/defaults.graven"))
        .schema(dir.join("schemas/common.graven"))
        .schema(dir.join("schemas/orders.graven"))
        .schema(dir.join("schemas/validation.graven"))
        .compile()
        .unwrap_or_else(|e| panic!("{e}"));

    graven_compiler::build::Build::new()
        .schema(dir.join("schemas/inventory.graven"))
        .client()
        .server()
        .compile()
        .unwrap_or_else(|e| panic!("{e}"));
}
