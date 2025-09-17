use std::env;
use std::path::PathBuf;

fn main() {
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    // Only generate bindings when the ffi feature is enabled
    #[cfg(feature = "ffi")]
    generate_c_header();
}

#[cfg(feature = "ffi")]
fn generate_c_header() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // Try to generate bindings using cbindgen
    let config = cbindgen::Config::from_root_or_default(&crate_dir);

    match cbindgen::generate_with_config(&crate_dir, config) {
        Ok(bindings) => {
            // Generate directly to target/debug/include
            let target_dir = PathBuf::from(env::var("OUT_DIR").unwrap())
                .ancestors()
                .nth(3)
                .unwrap()
                .to_path_buf();
            let target_include_dir = target_dir.join("include");

            if let Err(e) = std::fs::create_dir_all(&target_include_dir) {
                println!("cargo:warning=Failed to create {}: {}", target_include_dir.display(), e);
                return;
            }

            let header_path = target_include_dir.join("dbgif_protocol.h");
            bindings.write_to_file(&header_path);
        }
        Err(_) => {
            // cbindgen failed, continue without header generation
        }
    }
}


