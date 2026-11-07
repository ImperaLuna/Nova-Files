# Every check `nix flake check` runs, which is exactly what CI runs.
#
# Cargo checks build with the `ci` profile and reuse its dependency build
# from nix/build.nix. The checks in
# tools/ run as plain scripts over the source tree, with only the tools
# each one needs.
{
  pkgs,
  build,
  advisory-db,
}:
let
  inherit (build) craneLib ciArgs ciArtifacts;
  inherit (pkgs) lib;

  repoFiles = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../clippy.toml
      ../crates
      ../tools
      ../flake.nix
      ../nix
    ];
  };

  # A check that runs one tools/ script from the repo root.
  toolCheck =
    name: script: inputs:
    pkgs.runCommand "nova-files-${name}"
      {
        nativeBuildInputs = inputs;
      }
      ''
        cp -r ${repoFiles} source
        chmod -R u+w source
        cd source
        export HOME=$TMPDIR
        patchShebangs tools
        ${script}
        touch $out
      '';

  # cargo-deny reads its advisory database from a git checkout under
  # $CARGO_HOME/advisory-dbs, in a folder named after the database URL.
  # The sandbox has no network, so the pinned `advisory-db` input is copied
  # there and committed with the input's own date, which cargo-deny uses to
  # judge staleness: once the pin is 90 days old the check fails until
  # `nix flake update advisory-db` refreshes it. If cargo-deny renames the
  # folder, the check fails trying to fetch; update the name below.
  advisoryDbFolder = "advisory-db-3157b0e258782691";
in
{
  package = build.ciPackage;

  clippy = craneLib.cargoClippy (
    ciArgs
    // {
      cargoArtifacts = ciArtifacts;
      # --deny warnings: compiler warnings fail CI too.
      cargoClippyExtraArgs = "--workspace --all-targets -- --deny warnings";
    }
  );

  fmt = craneLib.cargoFmt { inherit (ciArgs) src pname version; };

  nextest = craneLib.cargoNextest (
    ciArgs
    // {
      cargoArtifacts = ciArtifacts;
      cargoNextestExtraArgs = "--workspace --no-tests=warn";
      # The D-Bus tests start a private dbus-daemon.
      nativeCheckInputs = [ pkgs.dbus ];
    }
  );

  deny = craneLib.cargoDeny (
    ciArgs
    // {
      cargoDenyChecks = "advisories bans licenses sources";
      cargoDenyExtraArgs = "--offline";
      nativeBuildInputs = [ pkgs.git ];
      preBuild = ''
        db=$CARGO_HOME/advisory-dbs/${advisoryDbFolder}
        mkdir -p "$db"
        cp -r ${advisory-db}/. "$db"
        chmod -R u+w "$db"
        export GIT_AUTHOR_DATE=@${toString advisory-db.lastModified}
        export GIT_COMMITTER_DATE=$GIT_AUTHOR_DATE
        git -C "$db" init --quiet
        git -C "$db" add --all
        git -C "$db" -c user.name=nix -c user.email=nix@localhost commit --quiet --message advisory-db
      '';
    }
  );

  machete = toolCheck "machete" "cargo-machete" [ pkgs.cargo-machete ];

  deps = toolCheck "deps" "tools/check-deps.sh" [
    pkgs.cargo
    pkgs.jq
  ];

  clippy-config = toolCheck "clippy-config" "tools/check-clippy-config.sh" [ ];

  file-length = toolCheck "file-length" "tools/check-file-length.sh" [ ];

  bare-numbers = toolCheck "bare-numbers" "ast-grep scan --config tools/ast-grep/sgconfig.yml" [
    pkgs.ast-grep
  ];

  nix-fmt = toolCheck "nix-fmt" "nixfmt --check flake.nix nix/*.nix" [ pkgs.nixfmt ];

  duplicates = toolCheck "duplicates" "tools/check-duplicates.sh" [ pkgs.pmd ];
}
