{
  description = "PDF Masking CLI Tool - Rust development environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            rust-bin.stable.latest.default

            # Build tools
            pkg-config
            cmake
            clang
            libclang

            # Libraries for pdf_masking
            leptonica
            jbig2enc
            qpdf
            libjpeg
            libpng

            # Development tools
            markdownlint-cli

            # Utilities
            git
          ];

          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
              pkgs.leptonica
              pkgs.jbig2enc
              pkgs.qpdf
              pkgs.libjpeg
              pkgs.libpng
              pkgs.libclang.lib
            ]}:$LD_LIBRARY_PATH"
            export PKG_CONFIG_PATH="${pkgs.leptonica}/lib/pkgconfig:$PKG_CONFIG_PATH"
            export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"
            export BINDGEN_EXTRA_CLANG_ARGS="''${BINDGEN_EXTRA_CLANG_ARGS:-} -I${pkgs.libclang.lib}/lib/clang/${pkgs.libclang.version}/include"
          '';
        };
      }
    );
}
