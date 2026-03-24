{
  description = "vigil";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      allSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems =
        f:
        nixpkgs.lib.genAttrs allSystems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              overlays = [
                rust-overlay.overlays.default
                self.overlays.default
              ];
            };
          }
        );
    in
    {
      overlays.default = final: _prev: {
        rustToolchain = final.rust-bin.stable.latest.default.override {
          extensions = [
            "clippy"
            "rustfmt"
            "rust-src"
          ];
        };
      };

      packages = forAllSystems (
        { pkgs, ... }:
        let
          rustPlatform = pkgs.makeRustPlatform {
            cargo = pkgs.rustToolchain;
            rustc = pkgs.rustToolchain;
          };

          vigil = rustPlatform.buildRustPackage {
            pname = "vigil";
            version = "0.1.0";
            src = self;

            cargoLock = {
              lockFile = self + /Cargo.lock;
            };

            nativeBuildInputs = [ pkgs.makeWrapper ];
            nativeCheckInputs = [ pkgs.git ];

            postFixup = ''
              wrapProgram "$out/bin/vigil" \
                --prefix PATH : ${pkgs.lib.makeBinPath [
                  pkgs.bash
                  pkgs.git
                ]}
            '';

            meta = {
              description = "Terminal UI for inspecting git changes";
              license = pkgs.lib.licenses.mit;
              mainProgram = "vigil";
            };
          };
        in
        {
          default = vigil;
          vigil = vigil;
        }
      );

      apps = forAllSystems (
        { system, ... }:
        {
          default = {
            type = "app";
            program = "${self.packages.${system}.default}/bin/vigil";
          };

          vigil = {
            type = "app";
            program = "${self.packages.${system}.vigil}/bin/vigil";
          };
        }
      );

      devShells = forAllSystems (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.git
              pkgs.rustToolchain
            ];
          };
        }
      );

      formatter = forAllSystems ({ pkgs, ... }: pkgs.nixfmt-rfc-style);
    };
}
