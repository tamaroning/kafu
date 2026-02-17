fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/service.proto");
    tonic_prost_build::compile_protos("proto/service.proto")?;
    Ok(())
}
