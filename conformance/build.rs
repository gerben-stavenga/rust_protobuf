use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Protos come from @protobuf Bazel module now
    println!("cargo:rerun-if-changed=BUILD.bazel");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(&manifest_dir).parent().unwrap().to_path_buf();

    // Get bazelisk path
    let bazelisk = protocrap_codegen::get_bazelisk_path(&workspace_root);

    // Build descriptor set via Bazel
    println!("cargo:warning=Building conformance protos via Bazel...");
    let status = Command::new(&bazelisk)
        .current_dir(&workspace_root)
        .args(["build", "//conformance:conformance_descriptor_set"])
        .status()
        .expect("Failed to run bazelisk");

    if !status.success() {
        panic!("Bazel build failed for conformance_descriptor_set");
    }

    let desc_file = workspace_root.join("bazel-bin/conformance/conformance_descriptor_set.bin");

    // Find the codegen binary - use the already-built one instead of cargo run
    let codegen_bin = if let Ok(path) = std::env::var("PROTOCRAP_CODEGEN") {
        PathBuf::from(path)
    } else {
        let debug_bin = workspace_root.join("target/debug/protocrap-codegen");
        let release_bin = workspace_root.join("target/release/protocrap-codegen");

        if release_bin.exists() {
            release_bin
        } else if debug_bin.exists() {
            debug_bin
        } else {
            panic!(
                "protocrap-codegen binary not found. Please build it first with: cargo build -p protocrap-codegen"
            );
        }
    };

    println!(
        "cargo:warning=Using codegen binary: {}",
        codegen_bin.display()
    );

    // Generate Rust code
    let out_file = format!("{}/conformance_all.pc.rs", out_dir);
    let status = Command::new(&codegen_bin)
        .args([desc_file.to_str().unwrap(), &out_file])
        .status()
        .expect("Failed to run protocrap-codegen");

    if !status.success() {
        panic!("protocrap-codegen failed");
    }

    println!("cargo:warning=Generated {}", out_file);

    // Copy descriptor set to OUT_DIR for include_bytes!
    // Remove existing file first (may be read-only from previous copy)
    let out_bin = format!("{}/conformance_all.bin", out_dir);
    let _ = std::fs::remove_file(&out_bin);
    let data = std::fs::read(&desc_file).expect("Failed to read descriptor set");
    std::fs::write(&out_bin, data).expect("Failed to write descriptor set");
}
