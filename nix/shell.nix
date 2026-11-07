# The dev shell: the toolchain and every check's tools, plus rust-analyzer
# for editors and agents, and `just` for the per-job commands.
{
  pkgs,
  build,
  checks,
}:
build.craneLib.devShell {
  inherit checks;
  # The checks' own tools come in through `checks`; these are the extras.
  packages = [
    pkgs.rust-analyzer
    pkgs.cargo-mutants
    pkgs.just
    # For the D-Bus tests outside `nix flake check`.
    pkgs.dbus
  ];
  # Turns on the repo's git hooks (tools/git-hooks), such as the one that
  # refuses non-code changes in a stacked branch before the push.
  shellHook = ''
    git config core.hooksPath tools/git-hooks 2>/dev/null || true
  '';
  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
  # GPUI loads these at run time; `cargo run` needs them as the package does.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath build.runtimeLibs;
}
