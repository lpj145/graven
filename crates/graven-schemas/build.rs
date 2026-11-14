//! Generates the code for `bank`, `game`, `iot`, `stream` and the evolution fixtures
//! (`evo/bank_v2`, `evo/bank_v3`) in `OUT_DIR` with the `graven-compiler` build API,
//! reading the committed locks (`schemas/**/<schema>.graven.lock`); it never writes them.
//! A missing or outdated lock ⇒ an error telling you to run `gravenc update <schema>`.
//!
//! The evolution fixtures are earlier and later versions of `bank.graven` (same namespace), so
//! each is compiled on its own: within one compilation, a namespace is declared by one file.

use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("schemas");
    let mut b = graven_compiler::build::Build::new();
    for s in ["bank", "game", "iot", "stream"] {
        b = b.schema(dir.join(format!("{s}.graven")));
    }
    b.compile().unwrap_or_else(|e| panic!("{e}"));
    for s in ["evo/bank_v2", "evo/bank_v3"] {
        graven_compiler::build::Build::new()
            .schema(dir.join(format!("{s}.graven")))
            .compile()
            .unwrap_or_else(|e| panic!("{e}"));
    }
}
