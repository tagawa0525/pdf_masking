fn main() {
    // Detect and link leptonica via pkg-config.
    // On NixOS the PKG_CONFIG_PATH is set by flake.nix so that
    // pkg-config can find lept.pc provided by the leptonica package.
    pkg_config::probe_library("lept").expect("leptonica not found via pkg-config");
}
