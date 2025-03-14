fn main() {
    capnpc::CompilerCommand::new()
        .file("schema/message.capnp")
        .run()
        .expect("compiling schema");
}
