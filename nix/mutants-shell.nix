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
    pkgs.mold
    # For the D-Bus tests.
    pkgs.dbus
  ];
  # Each mutant changes one line, so the build is mostly linking and
  # recompiling one crate: mold links several times faster than the default
  # linker, and incremental builds reuse the rest of the crate. Measured on
  # #10's mutants, a mutant went from about 10 s to about 4 s.
  RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
  CARGO_PROFILE_CI_INCREMENTAL = "true";
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath build.runtimeLibs;
}
