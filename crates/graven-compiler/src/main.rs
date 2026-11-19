//! `gravenc`: CLI of the `.graven` schema compiler and its data tools ([`graven_compiler::cli`]).

fn main() -> std::process::ExitCode {
    graven_compiler::cli::run(std::env::args_os())
}
