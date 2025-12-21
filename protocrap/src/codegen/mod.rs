// Conditionally declare the external crate
#[cfg(feature = "bootcrap")]
pub(crate) extern crate protocrap_stable;
use std::path::Path;

use anyhow::Result;
#[cfg(not(feature = "bootcrap"))]
pub(crate) use crate as protocrap;
#[cfg(feature = "bootcrap")]
pub(crate) use protocrap_stable as protocrap;

use protocrap::google::protobuf::FileDescriptorSet::ProtoType as FileDescriptorSet;
use protocrap::ProtobufExt;

mod generator;
mod names;
mod static_gen;
mod tables;

/// Generate Rust code from protobuf descriptor bytes
pub fn generate(descriptor_bytes: &[u8]) -> Result<String> {
    // Parse descriptor with prost
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let mut file_set = FileDescriptorSet::default();
    if !file_set.decode_flat::<100>(&mut arena, descriptor_bytes) {
        return Err(anyhow::anyhow!("Failed to decode file descriptor set"));
    }

    // Generate tokens
    let tokens = generator::generate_file_set(&file_set)?;

    // Pretty-print to string
    let syntax_tree = syn::parse2(tokens)?;
    Ok(prettyplease::unparse(&syntax_tree))
}

pub fn generate_proto(out_dir: &str, proto_file: &str, output_name: &str) -> Result<()> {
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
        return Err(anyhow::anyhow!("protoc failed for {}", proto_file));
    }

    // Read descriptor bytes
    let descriptor_bytes = std::fs::read(&desc_file)?;

    // Generate Rust code with protocrap-codegen
    let code = generate(&descriptor_bytes).map_err(|e| {
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
