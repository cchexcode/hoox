{
  description = "devenv";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url  = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rust = pkgs.makeRustPlatform {
          cargo = pkgs.rust-bin.stable."${versions.rust}".default;
          rustc = pkgs.rust-bin.stable."${versions.rust}".default;
        };
        versions = {
          rust = "1.82.0";
        };
      in with pkgs; {
        devShells.default = mkShell {
          buildInputs = [
            # pkg
            pkg-config
            # rust
            rust-bin.stable."${versions.rust}".default
            rust-analyzer
        ];

          shellHook = ''
            export RUST_LOG=debug
            export RUST_BACKTRACE=1

            # make sure hooks are installed
            cargo install --force --path .
            hoox init

            printf "Versions:\n"
            printf "$(rustc --version)\n"
            printf "$(cargo --version)\n"
            printf "$(rustfmt --version)\n"
            printf "\n"
          '';
        };
      }
    );
}
