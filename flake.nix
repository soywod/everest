{
  description = "Command-line interface and library for email synchronization";

  inputs = {
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, rust-overlay, ... }:
    utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [
              rust-overlay.overlay
              (self: super: {
                # Because rust-overlay bundles multiple rust packages into one
                # derivation, specify that mega-bundle here, so that crate2nix
                # will use them automatically.
                rustc = self.rust-bin.stable.latest.default;
                cargo = self.rust-bin.stable.latest.default;
              })
            ];
          };
        in
        rec {
          # nix develop
          devShell = pkgs.mkShell {
            RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
            buildInputs = with pkgs; [
              cargo
              cargo-watch
              trunk
              ripgrep
              rust-analyzer
              rustfmt
              rnix-lsp
              nixpkgs-fmt
            ];
          };
        }
      );
}
