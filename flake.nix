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
    inputs@{
      flake-parts,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      perSystem =
        {
          self',
          pkgs,
          inputs',
          ...
        }:
        let
          fenix = inputs'.fenix.packages;
          # rustChannel = "stable";
          toolchain = fenix.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-qqF33vNuAdU5vua96VKVIwuc43j4EFeEXbjQ6+l4mO4=";
          };
          rustplatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        {
          packages.sukr = rustplatform.buildRustPackage {
            pname = "sukr";
            version = "0.1.0";
            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };
            # Programs and libraries used at build-time
            nativeBuildInputs =
              with pkgs;
              # Left empty on purpose to easily add if needed
              [ ]
              ++ lib.optionals stdenv.isDarwin [
                apple-sdk
                libiconv
              ];
          };
          packages.default = self'.packages.sukr;

          # Default shell opened with `nix develop`
          devShells.default = pkgs.mkShell {
            name = "dev";

            # Available packages on https://search.nixos.org/packages
            buildInputs =
              with pkgs;
              [
                toolchain
                treefmt
                shfmt
                rust-analyzer
                taplo
                pkg-config
                nixfmt
                nodePackages.prettier
                miniserve # Dev server for testing
              ]
              ++ lib.optionals stdenv.isDarwin [
                apple-sdk
                libiconv
              ];

            shellHook = ''
              echo "Welcome to the rust devshell!"
            '';
          };
        };
    };
}
