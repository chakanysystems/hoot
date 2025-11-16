{
  description = "Hoot Email Client";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    nixGL.url = "github:nix-community/nixGL";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        basePkgs = with pkgs; [
          (
            rust-bin.stable.latest.default.override {
              extensions = [ "rust-src" "rust-analyzer" ];
            }
          )
          libiconv
          pkg-config
          fontconfig
        ];

        linuxPkgs = with pkgs; [
          wayland
          libxkbcommon
          libGL
        ];

        darwinPkgs = with pkgs; [
          apple-sdk
        ];

        allPkgs = basePkgs ++ (if pkgs.stdenv.isDarwin then darwinPkgs else []) ++ (if pkgs.stdenv.isLinux then linuxPkgs else []);
      in
        {
          devShells.default = with pkgs; mkShell {
            buildInputs = allPkgs;

            LD_LIBRARY_PATH =
              builtins.foldl' (a: b: "${a}:${b}/lib") "${pkgs.vulkan-loader}/lib" allPkgs;

            shellHook = ''
              echo "Welcome to the Hoot Devshell. You should be good to go!"
            '';
          };
        }
    );
}
