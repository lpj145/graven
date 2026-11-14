fn main() {
    capnpc::CompilerCommand::new().src_prefix("schemas").file("schemas/bench.capnp").run().expect("capnp compile");
    println!("cargo:rerun-if-changed=schemas/bench.capnp");
}
