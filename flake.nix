{
  description = "sukr - bespoke static site compiler";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    fenix,
  }: let
    systems = [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ];
    forAllSystems = fn: nixpkgs.lib.genAttrs systems (system: fn (pkgsFor system) (toolchainFor system));
    pkgsFor = system:
      import nixpkgs {
        system = system;
      };
    toolchainFor = system:
      fenix.packages.${system}.fromToolchainFile {
        file = ./rust-toolchain.toml;
        sha256 = "sha256-h+t2xTBz5yt2YIO+1VMIIGlCU7gyp2LYOFvaV1nwOXU=";
      };
    cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  in {
    devShells = forAllSystems (
      pkgs: toolchain: {
        default = pkgs.mkShell.override {stdenv = pkgs.clangStdenv;} {
          RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
          packages =
            [
              toolchain
              pkgs.treefmt
              pkgs.shfmt
              pkgs.rust-analyzer
              pkgs.taplo
              pkgs.pkg-config
              pkgs.nixfmt
              pkgs.nodePackages.prettier
              pkgs.miniserve # Dev server for testing
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk
              pkgs.libiconv
            ];
        };
      }
    );
    packages = forAllSystems (pkgs: toolchain: rec {
      sukr =
        let
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          src = ./.;
          cargoHash = "sha256-HrUYDzs/GFaQM4t8Jb2O/e1gRbyVHKmhlCnSdlwstP8=";
        };
      default = sukr;
    });
  };
}
