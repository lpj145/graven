#![no_main]
use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use graven_compiler::codegen::generate;
use graven_compiler::lock::lock_from_ir;
use graven_compiler::{check, parse};

/// Valid schemas from the repository, the starting point for mutations.
const SEEDS: &[&str] = &[
    include_str!("../../crates/graven-schemas/schemas/bank.graven"),
    include_str!("../../crates/graven-schemas/schemas/game.graven"),
    include_str!("../../crates/graven-schemas/schemas/iot.graven"),
    include_str!("../../crates/graven-schemas/schemas/stream.graven"),
    include_str!("../../crates/graven-conformance/schemas/conformance.graven"),
    include_str!("../../crates/graven-codegen-tests/schemas/edge.graven"),
    include_str!("../../crates/graven-codegen-tests/schemas/names.graven"),
];

/// An edit over the (whitespace-separated) tokens of a valid schema.
#[derive(Arbitrary, Debug)]
enum Edit {
    Delete(u16),
    Duplicate(u16),
    Swap(u16, u16),
    Replace(u16, String),
    Insert(u16, String),
    /// Nests a type in `[` … `]` `n` times (deep nesting).
    Nest(u16, u16),
}

fn mutated(u: &mut Unstructured<'_>) -> arbitrary::Result<String> {
    let seed = SEEDS[u.choose_index(SEEDS.len())?];
    let mut toks: Vec<String> = seed.split_inclusive(char::is_whitespace).map(String::from).collect();
    for _ in 0..u.int_in_range(1..=8)? {
        let n = toks.len().max(1);
        match Edit::arbitrary(u)? {
            Edit::Delete(i) if !toks.is_empty() => {
                toks.remove(i as usize % n);
            }
            Edit::Duplicate(i) if !toks.is_empty() => {
                let t = toks[i as usize % n].clone();
                toks.insert(i as usize % n, t);
            }
            Edit::Swap(a, b) if !toks.is_empty() => toks.swap(a as usize % n, b as usize % n),
            Edit::Replace(i, s) if !toks.is_empty() => toks[i as usize % n] = s,
            Edit::Insert(i, s) => toks.insert(i as usize % (n + 1).min(toks.len() + 1), s),
            Edit::Nest(i, k) if !toks.is_empty() => {
                let k = (k % 2048) as usize;
                let t = &toks[i as usize % n];
                toks[i as usize % n] = format!("{}{}{}", "[".repeat(k), t.trim_end(), "]".repeat(k)) + " ";
            }
            _ => {}
        }
    }
    Ok(toks.concat())
}

fn compile(src: &str) {
    let Ok(ast) = parse(src) else { return };
    let Ok(ir) = check(&ast, &|_: &str, _: u16| None) else { return };
    let lock = lock_from_ir(&ir, None);
    let _ = generate(&ir, &lock);
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    // Half: arbitrary text; half: a mutation of a valid schema.
    if u.arbitrary::<bool>().unwrap_or(false) {
        if let Ok(src) = mutated(&mut u) {
            compile(&src);
        }
    } else if let Ok(src) = std::str::from_utf8(u.take_rest()) {
        compile(src);
    }
});
