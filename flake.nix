{
  description = "sukr - bespoke static site compiler";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = fn: nixpkgs.lib.genAttrs systems (system: fn system);
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          fenixPkgs = fenix.packages.${system};
          toolchain = fenixPkgs.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-mvUGEOHYJpn3ikC5hckneuGixaC+yGrkMM/liDIDgoU=";
          };
        in
        {
          default = pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
            packages = [
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
    };
}
