{
  description = "Nova Files: a fast, rounded file manager for Linux, built on GPUI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: {
        default = pkgs.callPackage ./nix/package.nix { };
        nova-files = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      });

      # `nixpkgs.overlays = [ nova-files.overlays.default ];` then add
      # `pkgs.nova-files` to your packages.
      overlays.default = final: _prev: {
        nova-files = final.callPackage ./nix/package.nix { };
      };

      devShells = forAllSystems (
        pkgs:
        let
          nova-files = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ nova-files ];
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              rust-analyzer
            ];
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath nova-files.runtimeLibs;
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt-rfc-style);
    };
}
