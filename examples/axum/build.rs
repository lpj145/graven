// Generates `$OUT_DIR/shop.rs` from the schema: types, the server edge (`ShopCall`) and,
// for the test in `src/main.rs`, the Rust client. It reads the lockfile and never writes it.
fn main() {
    graven_compiler::build::Build::new()
        .schema("schemas/shop.graven")
        .server()
        .client()
        .compile()
        .unwrap_or_else(|e| panic!("{e}"));
}
