use std::env;

fn main() {
    // --- jbig2enc C++ shim ---
    let jbig2enc_include = env::var("JBIG2ENC_INCLUDE_PATH")
        .expect("JBIG2ENC_INCLUDE_PATH must be set (see flake.nix)");
    let jbig2enc_lib =
        env::var("JBIG2ENC_LIB_PATH").expect("JBIG2ENC_LIB_PATH must be set (see flake.nix)");
    let leptonica_include = env::var("LEPTONICA_INCLUDE_PATH")
        .expect("LEPTONICA_INCLUDE_PATH must be set (see flake.nix)");

    // Compile the thin C++ shim that wraps jbig2_encode_generic with extern "C" linkage
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("csrc/jbig2enc_shim.cpp")
        .include(&jbig2enc_include)
        .include(&leptonica_include)
        .compile("jbig2enc_shim");

    // Link against jbig2enc shared library
    println!("cargo:rustc-link-search=native={jbig2enc_lib}");
    println!("cargo:rustc-link-lib=jbig2enc");

    // jbig2enc is C++, so we need the C++ standard library
    println!("cargo:rustc-link-lib=stdc++");

    // Re-run if the shim source changes
    println!("cargo:rerun-if-changed=csrc/jbig2enc_shim.cpp");
    println!("cargo:rerun-if-env-changed=JBIG2ENC_INCLUDE_PATH");
    println!("cargo:rerun-if-env-changed=JBIG2ENC_LIB_PATH");
    println!("cargo:rerun-if-env-changed=LEPTONICA_INCLUDE_PATH");
}
