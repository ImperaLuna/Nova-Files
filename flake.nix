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
    # An earlier Nova Files commit whose dependency build the current one
    # starts from, so a Cargo.lock change rebuilds only the crates it
    # touches (nix/build.nix). After a GPUI bump lands on main, point the
    # rev at that main commit and run `nix flake update deps-base`.
    deps-base = {
      url = "github:ImperaLuna/Nova-Files/c8ae1f63baaa195a859831fb7b2fd471131c17ab";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      advisory-db,
      deps-base,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system});
      build = forEachSystem (
        _system: pkgs:
        import ./nix/build.nix {
          inherit pkgs crane;
          depsBase = import "${deps-base}/nix/build.nix" { inherit pkgs crane; };
        }
      );
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
