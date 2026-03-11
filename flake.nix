{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "nixpkgs/nixos-unstable";
  };

  outputs =
    {
      self,
      flake-utils,
      fenix,
      nixpkgs,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ fenix.overlays.default ];
        };
      in
      {
        formatter = pkgs.nixfmt-tree;

        packages = {
          default = self.packages.${system}.zeroclaw;
          zeroclaw-web = pkgs.callPackage ./web/package.nix { };
          zeroclaw = pkgs.callPackage ./package.nix {
            inherit (self.packages.${system}) zeroclaw-web;
            rustToolchain = pkgs.fenix.stable.withComponents [
              "cargo"
              "clippy"
              "rust-src"
              "rustc"
              "rustfmt"
            ];
          };
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.zeroclaw ];
          packages = [ pkgs.rust-analyzer ];
        };
      }
    )
    // {
      overlays.default = (
        final: prev: {
          inherit (self.packages.${final.system}) zeroclaw zeroclaw-web;
        }
      );

      nixosModules = {
        default = self.nixosModules.zeroclaw;
        zeroclaw = ./nixos/module.nix;
      };

    };
}
