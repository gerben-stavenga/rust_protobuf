// build.rs

use std::io::Result;

fn main() -> Result<()> {
    let out_dir = std::env::var("OUT_DIR").unwrap();

    println!("cargo:rerun-if-changed=proto/test.proto");

    // Generate prost version (for comparison)
    println!("cargo:warning=Generating prost version...");
    prost_build::Config::new()
        .out_dir(&out_dir)
        .compile_protos(
            &["../codegen/codegen-tests/proto/test.proto"],
            &["../codegen/codegen-tests/proto/"],
        )?;

    Ok(())
}
