// build.rs

use std::io::Result;
use std::path::Path;

fn main() -> Result<()> {
    let out_dir = std::env::var("OUT_DIR").unwrap();

    println!("cargo:rerun-if-changed=proto/test.proto");

    // Generate prost version (for comparison)
    println!("cargo:warning=Generating prost version...");
    prost_build::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&["proto/test.proto"], &["proto/"])?;

    // Generate protocrap version with Rust codegen
    println!("cargo:warning=Generating protocrap version with Rust codegen...");

    // Generate test.proto
    generate_proto(&out_dir, "proto/test.proto", "test.pc.rs")?;

    Ok(())
}

fn generate_proto(out_dir: &str, proto_file: &str, output_name: &str) -> Result<()> {
    let desc_file = format!("{}/temp.desc", out_dir);
    let output_file = format!("{}/{}", out_dir, output_name);

    // Determine proto path (directory containing the proto file)
    let proto_path = Path::new(proto_file).parent().unwrap_or(Path::new("."));

    // Generate descriptor set with protoc
    let status = std::process::Command::new("protoc")
        .args(&[
            "--descriptor_set_out",
            &desc_file,
            "--include_imports",
            &format!("--proto_path={}", proto_path.display()),
            proto_file,
        ])
        .status()?;

    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("protoc failed for {}", proto_file),
        ));
    }

    // Read descriptor bytes
    let descriptor_bytes = std::fs::read(&desc_file)?;

    // Generate Rust code with protocrap-codegen
    let code = protocrap::codegen::generate(&descriptor_bytes).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Code generation failed: {}", e),
        )
    })?;

    // Write output
    std::fs::write(&output_file, code)?;

    println!("cargo:warning=✅ Generated {}", output_file);

    Ok(())
}
