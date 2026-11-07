# Nova Files: the package, the dev shell and every check.
#
# `nix flake check` runs everything CI runs (nix/checks.nix), so a green run
# here is a green run on GitHub. `nix develop` gives the same tools plus
# rust-analyzer and `just`, whose recipes run one job, optionally for one
# crate (see the justfile). CI's mutation job uses the slimmer
# `nix develop .#mutants`.
{
  description = "Nova Files, a fast file manager for Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      advisory-db,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system});
      build = forEachSystem (_system: pkgs: import ./nix/build.nix { inherit pkgs crane; });
    in
    {
      packages = forEachSystem (
        system: _pkgs: {
          default = build.${system}.package;
        }
      );

      overlays.default = final: _prev: {
        nova-files = self.packages.${final.stdenv.hostPlatform.system}.default;
      };

      checks = forEachSystem (
        system: pkgs:
        import ./nix/checks.nix {
          inherit pkgs advisory-db;
          build = build.${system};
        }
      );

      devShells = forEachSystem (
        system: pkgs: {
          default = import ./nix/shell.nix {
            inherit pkgs;
            build = build.${system};
            checks = self.checks.${system};
          };
          mutants = import ./nix/mutants-shell.nix {
            inherit pkgs;
            build = build.${system};
          };
        }
      );

      formatter = forEachSystem (_system: pkgs: pkgs.nixfmt);
    };
}
