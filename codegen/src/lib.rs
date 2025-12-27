// protocrap-codegen/src/lib.rs
#![feature(allocator_api)]
use std::path::Path;

use anyhow::Result;
#[cfg(not(feature = "bootcrap"))]
use protocrap;
use protocrap::ProtobufExt;
use protocrap::google::protobuf::FileDescriptorSet::ProtoType as FileDescriptorSet;
#[cfg(feature = "bootcrap")]
use protocrap_stable as protocrap;

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

    let should_pretty_print = true;
    if should_pretty_print {
        // Pretty-print to string
        let syntax_tree = syn::parse2(tokens)?;
        Ok(prettyplease::unparse(&syntax_tree))
    } else {
        Ok(tokens.to_string())
    }
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

    println!("cargo:warning=Generated {}", output_file);

    Ok(())
}

const BAZELISK_VERSION: &str = "1.25.0";

/// Returns path to bazelisk binary, downloading if necessary
pub fn get_bazelisk_path(_workspace_root: &Path) -> std::path::PathBuf {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        panic!("Unsupported OS for bazelisk")
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        panic!("Unsupported arch for bazelisk")
    };

    let ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };

    let filename = format!("bazelisk-{}-{}{}", os, arch, ext);

    // Cache in ~/.cache/protocrap/
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from(".cache"))
        .join("protocrap")
        .join(format!("bazelisk-{}", BAZELISK_VERSION));

    let bazelisk_path = cache_dir.join(&filename);

    if !bazelisk_path.exists() {
        std::fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");

        let url = format!(
            "https://github.com/bazelbuild/bazelisk/releases/download/v{}/{}",
            BAZELISK_VERSION, filename
        );

        eprintln!("Downloading bazelisk from {}...", url);

        let output = std::process::Command::new("curl")
            .args(["-fsSL", "-o", bazelisk_path.to_str().unwrap(), &url])
            .output()
            .expect("Failed to run curl");

        if !output.status.success() {
            panic!(
                "Failed to download bazelisk: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bazelisk_path)
                .expect("Failed to get file metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bazelisk_path, perms)
                .expect("Failed to set executable permission");
        }

        eprintln!("Downloaded bazelisk to {:?}", bazelisk_path);
    }

    bazelisk_path
}

/// Build a Bazel target and return path to output file in bazel-bin
pub fn build_bazel_target(target: &str, workspace_root: &Path) -> Result<std::path::PathBuf> {
    let bazelisk = get_bazelisk_path(workspace_root);

    let output = std::process::Command::new(&bazelisk)
        .current_dir(workspace_root)
        .args(["build", target])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Bazel build failed for {}: {}",
            target,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Parse target to get output path: //:foo -> bazel-bin/foo
    let output_name = target
        .trim_start_matches("//")
        .trim_start_matches(':')
        .replace(':', "/");

    Ok(workspace_root.join("bazel-bin").join(output_name))
}

/// Generate Rust code from a Bazel descriptor set target
pub fn generate_from_bazel_target(
    target: &str,
    output_file: &Path,
    workspace_root: &Path,
) -> Result<()> {
    // Build the descriptor set
    let desc_path = build_bazel_target(target, workspace_root)?;

    // Read descriptor bytes
    let descriptor_bytes = std::fs::read(&desc_path)?;

    // Generate Rust code
    let code = generate(&descriptor_bytes)?;

    // Write output
    std::fs::write(output_file, code)?;

    println!("cargo:warning=Generated {} from {}", output_file.display(), target);

    Ok(())
}
