//! Build script for the `engine-ffi` crate.
//!
//! The Rust-side FFI glue (the `uniffi_*` extern functions and the metadata the
//! bindings are generated from) is produced by the `uniffi::setup_scaffolding!`
//! macro at compile time, so a normal build needs no extra step.
//!
//! When `ENGINE_FFI_GEN_SWIFT` is set, this script generates the Swift wrapper,
//! C header, and module map from the **already-built** cdylib. It invokes the
//! compiled `uniffi-bindgen` executable directly (it is built by the same
//! `cargo build --features cli` that produces the cdylib) rather than spawning a
//! nested `cargo`, which would deadlock on the build lock. Because the dylib
//! does not exist yet on the very first build, generation is a no-op then and
//! runs on a subsequent build once the dylib is present (or via the explicit
//! `uniffi-bindgen generate` command used by CI).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");

    if env::var("ENGINE_FFI_GEN_SWIFT").is_err() {
        return;
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .expect("engine-ffi must live inside the workspace root")
        .to_path_buf();

    // Locate the cdylib. `build.rs` always runs on the host, so use the host
    // dynamic-library convention.
    let prefix = std::env::consts::DLL_PREFIX;
    let ext = std::env::consts::DLL_EXTENSION;

    let dylib = find(&root, &format!("{prefix}engine_ffi.{ext}"));
    let exe = find(&root, "uniffi-bindgen");

    let (dylib, exe) = match (dylib, exe) {
        (Some(d), Some(e)) => (d, e),
        _ => {
            eprintln!(
                "engine-ffi: skipping Swift generation (need both the built \
                 cdylib and the `uniffi-bindgen` executable; run `cargo build \
                 --features cli` first)"
            );
            return;
        }
    };

    let out_dir = env::var("ENGINE_FFI_SWIFT_OUT")
        .unwrap_or_else(|_| "generated/swift".to_string());
    std::fs::create_dir_all(&out_dir).ok();

    let status = Command::new(&exe)
        .args([
            "generate",
            dylib.to_str().unwrap(),
            "--language",
            "swift",
            "-o",
            &out_dir,
        ])
        .status()
        .expect("failed to run uniffi-bindgen");
    assert!(status.success(), "uniffi-bindgen failed");
    eprintln!("engine-ffi: generated Swift bindings into {out_dir}");
}

/// Search `target/<profile>/` (and `target/<triple>/<profile>/`) for `name`.
fn find(root: &PathBuf, name: &str) -> Option<PathBuf> {
    for profile in ["release", "debug"] {
        // Default (no --target) layout.
        let p = root.join("target").join(profile).join(name);
        if p.exists() {
            return Some(p);
        }
        // Per-target-triple layout (when built with --target).
        if let Ok(entries) = std::fs::read_dir(root.join("target")) {
            for e in entries.flatten() {
                let candidate = e.path().join(profile).join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}
