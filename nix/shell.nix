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
  ];
  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
}
