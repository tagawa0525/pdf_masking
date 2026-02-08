use std::env;

/// Helper to read a required environment variable, emitting a clear
/// `cargo:warning` before panicking so the user sees actionable guidance.
fn require_env(name: &str) -> String {
    match env::var(name) {
        Ok(v) => v,
        Err(_) => {
            println!(
                "cargo:warning={name} is not set. \
                 Build inside `nix develop` or set the variable manually. \
                 See flake.nix for details."
            );
            panic!("{name} is not set");
        }
    }
}

fn main() {
    // --- jbig2enc C++ shim ---
    let jbig2enc_include = require_env("JBIG2ENC_INCLUDE_PATH");
    let jbig2enc_lib = require_env("JBIG2ENC_LIB_PATH");
    let leptonica_include = require_env("LEPTONICA_INCLUDE_PATH");

    // Compile the thin C++ shim that wraps jbig2_encode_generic with extern "C" linkage
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("csrc/jbig2enc_shim.cpp")
        .include(&jbig2enc_include)
        .include(&leptonica_include)
        // Nixのg++ラッパーが-D_FORTIFY_SOURCE=3を注入するが、
        // デバッグビルド（-O なし）では機能しないためglibcが#warningを出す。
        // ラッパー側のフラグは制御できないため、#warningディレクティブを抑制する。
        .flag("-Wno-cpp")
        .compile("jbig2enc_shim");

    // Link against jbig2enc shared library
    println!("cargo:rustc-link-search=native={jbig2enc_lib}");
    println!("cargo:rustc-link-lib=jbig2enc");

    // jbig2enc is C++, so we need the C++ standard library.
    // The correct library name depends on the target platform.
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if !target.contains("msvc") {
        println!("cargo:rustc-link-lib=stdc++");
    }
    // MSVC links the C++ runtime automatically; no explicit flag needed.

    // Re-run if the shim source changes
    println!("cargo:rerun-if-changed=csrc/jbig2enc_shim.cpp");
    println!("cargo:rerun-if-env-changed=JBIG2ENC_INCLUDE_PATH");
    println!("cargo:rerun-if-env-changed=JBIG2ENC_LIB_PATH");
    println!("cargo:rerun-if-env-changed=LEPTONICA_INCLUDE_PATH");
}
