{
  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    devenv.url = "github:cachix/devenv";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixgl = {
      url = "github:nix-community/nixGL";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
    };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      devenv,
      rust-overlay,
      flake-parts,
      nixgl,
      crane,
      advisory-db,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } (
      top@{ config, moduleWithSystem, ... }:
      {
        systems = [
          "x86_64-linux"
          "aarch64-linux"
          "x86_64-darwin"
          "aarch64-darwin"
        ];

        perSystem =
          { config, system, ... }:
          let
            overlays = [
              (import rust-overlay)
              nixgl.overlay
            ];
            pkgs = import nixpkgs {
              inherit system overlays;
            };

            rustToolchain = pkgs.rust-bin.stable.latest.default.override {
              extensions = [
                "rust-src"
                "rust-analyzer"
              ];
            };

            craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

            rustPlatform = pkgs.makeRustPlatform {
              cargo = rustToolchain;
              rustc = rustToolchain;
            };

            src = craneLib.cleanCargoSource ./.;

            commonArgs = {
              inherit src;
              strictDeps = true;

              nativeBuildInputs = with pkgs; [
                pkg-config
                perl
                makeWrapper
              ];

              buildInputs =
                with pkgs;
                [
                  fontconfig
                ]
                ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
                  wayland
                  libxkbcommon
                  libGL
                  xorg.libX11
                  xorg.libXcursor
                  xorg.libXrandr
                  xorg.libXi
                ]
                ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                  darwin.apple_sdk.frameworks.AppKit
                  darwin.apple_sdk.frameworks.CoreGraphics
                  darwin.apple_sdk.frameworks.Foundation
                ];
            };

            # Use rustPlatform for building to avoid crane's git dependency issues
            hoot = craneLib.buildPackage {
              inherit src;
              inherit (commonArgs) nativeBuildInputs buildInputs;

              postInstall = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
                wrapProgram $out/bin/hoot \
                  --prefix LD_LIBRARY_PATH : "${
                    pkgs.lib.makeLibraryPath [
                      pkgs.wayland
                      pkgs.libxkbcommon
                      pkgs.libGL
                      pkgs.xorg.libX11
                      pkgs.xorg.libXcursor
                      pkgs.xorg.libXrandr
                      pkgs.xorg.libXi
                    ]
                  }"
              '';
            };

            # Keep crane artifacts for checks only
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            nixgl-wrapper = pkgs.writeShellScriptBin "hoot-nixgl" ''
              exec ${pkgs.nixgl.nixGLIntel}/bin/nixGLIntel ${hoot}/bin/hoot "$@"
            '';
          in
          {
            checks = {
              inherit hoot;

              hoot-clippy = craneLib.cargoClippy (
                commonArgs
                // {
                  inherit cargoArtifacts;
                  cargoClippyExtraArgs = "--all-targets -- --deny warnings";
                }
              );

              hoot-doc = craneLib.cargoDoc (
                commonArgs
                // {
                  inherit cargoArtifacts;
                  env.RUSTDOCFLAGS = "--deny warnings";
                }
              );

              hoot-fmt = craneLib.cargoFmt {
                inherit src;
              };

              hoot-toml-fmt = craneLib.taploFmt {
                src = pkgs.lib.sources.sourceFilesBySuffices src [ ".toml" ];
              };

              hoot-audit = craneLib.cargoAudit {
                inherit src advisory-db;
              };

              hoot-deny = craneLib.cargoDeny {
                inherit src;
              };
            };
            packages = rec {
              devenv-up = config.devShells.default.config.procfileScript;
              devenv-test = config.devShells.default.config.test;
              default = nixgl-wrapper;
              hoot-unwrapped = hoot;
            };

            formatter = pkgs.nixfmt-tree;

            apps.default = {
              type = "app";
              program = "${nixgl-wrapper}/bin/hoot-nixgl";
            };

            devShells.default = devenv.lib.mkShell {
              inherit inputs pkgs;
              modules = [
                (
                  { pkgs, ... }:
                  {
                    packages = [
                      rustToolchain
                    ];
                  }
                )
              ];
            };
          };
      }
    );
}
