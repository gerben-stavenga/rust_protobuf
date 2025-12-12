// build.rs
use std::io::Result;
use std::path::Path;

fn main() -> Result<()> {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    
    println!("cargo:rerun-if-changed=proto/test.proto");
    
    // Generate prost version
    println!("cargo:warning=Generating prost version...");
    prost_build::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&["proto/test.proto"], &["proto/"])?;
    
    // Generate protocrap version with Bazel
    println!("cargo:warning=Generating protocrap version...");
    
    let bazel_cmd = find_bazel_command()?;
    
    let status = std::process::Command::new(&bazel_cmd)
        .args(&["build", "//proto:test_protocrap", "//src:descriptor"])
        .status()?;
    
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Bazel build failed",
        ));
    }
    

    let bazel_out = "bazel-bin/proto/test.pc.rs";
    let dest = format!("{}/test.pc.rs", out_dir);

    for (bazel_out, dest) in [(bazel_out, &dest), ("bazel-bin/external/protobuf+/src/google/protobuf/descriptor.pc.rs", &format!("{}/descriptor.pc.rs", out_dir))] {
        copy_bazel_output(bazel_out, dest)?;
    }
    
    println!("cargo:warning=Copied {} to {}", bazel_out, dest);
    
    Ok(())
}

fn copy_bazel_output(src: &str, dest: &str) -> Result<()> {
    if !Path::new(src).exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Bazel didn't generate {}", src),
        ));
    }
    
    // Remove destination file if it exists (fixes permission issues)
    if Path::new(dest).exists() {
        std::fs::remove_file(dest).ok(); // Ignore errors if file doesn't exist
    }
    
    // Now copy (or read+write)
    std::fs::copy(src, dest)?;
    Ok(())
}

fn find_bazel_command() -> Result<String> {
    let candidates = vec!["bazelisk", "bazelisk-linux-amd64", "bazel"];
    
    for cmd in candidates {
        if std::process::Command::new(cmd)
            .arg("--version")
            .output()
            .is_ok()
        {
            return Ok(cmd.to_string());
        }
    }
    
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Could not find bazel or bazelisk",
    ))
}
