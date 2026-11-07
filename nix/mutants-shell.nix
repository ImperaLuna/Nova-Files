# The slim shell CI's mutation job runs in: the toolchain, the libraries the
# build links, cargo-nextest, cargo-mutants and `just`. It leaves out the
# other checks' tools (PMD's Java, ast-grep, rust-analyzer), which that job
# never runs and would otherwise download on every run.
{
  pkgs,
  build,
}:
build.craneLib.devShell {
  inputsFrom = [ build.ciPackage ];
  packages = [
    pkgs.cargo-nextest
    pkgs.cargo-mutants
    pkgs.just
    # For the D-Bus tests.
    pkgs.dbus
  ];
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath build.runtimeLibs;
}
