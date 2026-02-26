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

            # Libraries for pdf_masking
            qpdf
            pdfium-binaries

            # Development tools
            markdownlint-cli

            # Utilities
            git
          ];

          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
              pkgs.qpdf
              pkgs.pdfium-binaries
            ]}:$LD_LIBRARY_PATH"

            # pdfium path for pdfium-render dynamic loading
            export PDFIUM_DYNAMIC_LIB_PATH="${pkgs.pdfium-binaries}/lib"
          '';
        };
      }
    );
}
