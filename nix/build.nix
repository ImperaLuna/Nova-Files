# The Rust build, shared by the package, the checks and the dev shell.
#
# Crane builds the dependencies once (`cargoArtifacts`) and every Cargo
# check reuses them, so a change to our code never rebuilds the crates we
# borrow.
{ pkgs, crane }:
let
  craneLib = crane.mkLib pkgs;
  src = craneLib.cleanCargoSource ../.;

  commonArgs = {
    inherit src;
    strictDeps = true;
    pname = (pkgs.lib.importTOML ../crates/nova-files/Cargo.toml).package.name;
    inherit ((pkgs.lib.importTOML ../Cargo.toml).workspace.package) version;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
{
  inherit
    craneLib
    src
    commonArgs
    cargoArtifacts
    ;

  package = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      # Tests run once, in the nextest check, not again in the package.
      doCheck = false;
      meta = {
        description = "A fast file manager for Linux";
        homepage = "https://github.com/ImperaLuna/Nova-Files";
        license = pkgs.lib.licenses.mit;
        mainProgram = "nova-files";
        platforms = pkgs.lib.platforms.linux;
      };
    }
  );
}
