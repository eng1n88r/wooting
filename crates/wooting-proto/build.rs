fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=proto/wooting.proto");
    prost_build::Config::new()
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_protos(&["proto/wooting.proto"], &["proto"])
}
