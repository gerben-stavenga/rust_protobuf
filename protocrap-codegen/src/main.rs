// protocrap-codegen/src/main.rs

use std::fs;
use std::io::{self, Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        return Ok(());
    }

    // Read descriptor bytes
    let descriptor_bytes = if args[1] == "-" {
        // Read from stdin
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        buf
    } else {
        // Read from file
        fs::read(&args[1])?
    };

    println!("✅ Read descriptor ({} bytes)", descriptor_bytes.len());

    // Generate code
    let code = protocrap_codegen::generate(&descriptor_bytes)?;

    // Write output
    if args.len() > 2 {
        // Write to file
        fs::write(&args[2], code)?;
        eprintln!("✅ Generated {}", args[2]);
    } else {
        // Write to stdout
        io::stdout().write_all(code.as_bytes())?;
    }

    Ok(())
}

fn print_usage(program: &str) {
    eprintln!("Protocrap Code Generator");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  {} <descriptor.pb> [output.rs]", program);
    eprintln!("  {} - < descriptor.pb > output.rs", program);
    eprintln!();
    eprintln!("ARGUMENTS:");
    eprintln!("  descriptor.pb   FileDescriptorSet from protoc");
    eprintln!("  output.rs       Output Rust file (default: stdout)");
    eprintln!();
    eprintln!("EXAMPLE:");
    eprintln!("  protoc --descriptor_set_out=desc.pb --include_imports my.proto");
    eprintln!("  {} desc.pb my.pc.rs", program);
    eprintln!();
    eprintln!("  # Or use in pipeline:");
    eprintln!(
        "  protoc --descriptor_set_out=- --include_imports my.proto | {} -",
        program
    );
}
