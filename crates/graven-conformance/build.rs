//! Generates `conformance.rs` (`gravenc` code for `schemas/conformance.graven`) in `OUT_DIR`,
//! for the vector gate (`tests/generated.rs`). Reads the committed lock
//! (`schemas/conformance.graven.lock`); never writes it.

fn main() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    graven_compiler::build::Build::new()
        .schema(dir.join("schemas/conformance.graven"))
        .compile()
        .unwrap_or_else(|e| panic!("{e}"));
}
