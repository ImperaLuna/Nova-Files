# The Rust build, shared by the package, the checks and the dev shell.
#
# Crane builds the dependencies once (`cargoArtifacts`) and every Cargo
# check reuses them, so a change to our code never rebuilds the crates we
# borrow.
{ pkgs, crane }:
let
  craneLib = crane.mkLib pkgs;
  src = craneLib.cleanCargoSource ../.;
  appId = (pkgs.lib.importTOML ../.cargo/config.toml).env.NOVA_FILES_APP_ID;

  # GPUI opens these with dlopen at run time, so they go on the library path
  # of the wrapped binary and of tests, not only on the link line.
  runtimeLibs = with pkgs; [
    vulkan-loader
    wayland
    libxkbcommon
    libGL
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
  ];

  # Hashes of the git dependencies, so they are fetched as ordinary store
  # paths that Cachix serves, not cloned at evaluation time on every run
  # (the zed fork's clone alone took a minute of each CI job). After
  # changing a git dependency, set its hash to `pkgs.lib.fakeHash` and copy
  # the real one from the build error.
  outputHashes = {
    "git+https://github.com/ImperaLuna/zed?rev=c6828e8dc23590aad9c69d5fb03a8d5c799b12cd#c6828e8dc23590aad9c69d5fb03a8d5c799b12cd" =
      "sha256-pQzBnSp2+WNlBDlnS9A9Q84BPGQngX/vaBu9N91BYWo=";
    "git+https://github.com/proptest-rs/proptest?rev=3dca198a8fef1b32e3a66f1e1897c955b4dc5b5b#3dca198a8fef1b32e3a66f1e1897c955b4dc5b5b" =
      "sha256-p5NTcHhruI8QQvANACg8AMRVNmuvGxs2NLit+/8PaWo=";
    "git+https://github.com/zed-industries/font-kit?rev=94b0f28166665e8fd2f53ff6d268a14955c82269#94b0f28166665e8fd2f53ff6d268a14955c82269" =
      "sha256-KXygi0olNQi5yM8eaJVykNDtbPMDjT+cWPBF8UrtXR4=";
    "git+https://github.com/zed-industries/scap?rev=4afea48c3b002197176fb19cd0f9b180dd36eaac#4afea48c3b002197176fb19cd0f9b180dd36eaac" =
      "sha256-BihiQHlal/eRsktyf0GI3aSWsUCW7WcICMsC2Xvb7kw=";
    "git+https://github.com/zed-industries/wgpu.git?branch=v29#357a0c56e0070480ad9daea5d2eaa83150b79e88" =
      "sha256-jwPdrd2XLvK5ddEutR/39OLMh2JU3UXNWIcJKCndh+U=";
    "git+https://github.com/zed-industries/xim-rs.git?rev=16f35a2c881b815a2b6cdfd6687988e84f8447d8#16f35a2c881b815a2b6cdfd6687988e84f8447d8" =
      "sha256-pRT4Sz1JU9ros47/7pmIW9kosWOGMOItcnNd+VrvnpE=";
  };

  commonArgs = {
    inherit src outputHashes;
    strictDeps = true;
    nativeBuildInputs = with pkgs; [
      pkg-config
      cmake
      makeWrapper
      rustPlatform.bindgenHook
    ];
    buildInputs = with pkgs; [
      fontconfig
      freetype
      libxkbcommon
      wayland
      libx11
      libxcb
      zstd
    ];
    LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
    pname = (pkgs.lib.importTOML ../crates/nova-files/Cargo.toml).package.name;
    inherit ((pkgs.lib.importTOML ../Cargo.toml).workspace.package) version;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # The checks build with the `ci` profile from Cargo.toml (no LTO, no debug
  # info) and share one dependency build, so a PR never waits on release
  # linking. The release build is only for the package people install.
  ciArgs = commonArgs // {
    CARGO_PROFILE = "ci";
  };
  ciArtifacts = craneLib.buildDepsOnly ciArgs;

  # The installed app: binaries, the D-Bus service file and the wrapper.
  mkPackage =
    args: artifacts:
    craneLib.buildPackage (
      args
      // {
        cargoArtifacts = artifacts;
        # Tests run once, in the nextest check, not again in the package.
        doCheck = false;
        postInstall = ''
          install -Dm644 ${../data/dbus-service.in} \
            $out/share/dbus-1/services/${appId}.service
          substituteInPlace $out/share/dbus-1/services/${appId}.service \
            --replace-fail "@app_id@" "${appId}" \
            --replace-fail "@bin@" "$out/bin/nova-files"
          # Launchers list the app from this file. It is named after the app
          # id, as DBusActivatable requires.
          install -Dm644 ${../data/desktop-entry.in} \
            $out/share/applications/${appId}.desktop
          substituteInPlace $out/share/applications/${appId}.desktop \
            --replace-fail "@bin@" "$out/bin/nova-files"
        '';
        postFixup = ''
          wrapProgram $out/bin/nova-files \
            --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtimeLibs}
        '';
        meta = {
          description = "A fast file manager for Linux";
          homepage = "https://github.com/ImperaLuna/Nova-Files";
          license = pkgs.lib.licenses.mit;
          mainProgram = "nova-files";
          platforms = pkgs.lib.platforms.linux;
        };
      }
    );
in
{
  inherit
    craneLib
    src
    commonArgs
    cargoArtifacts
    ciArgs
    ciArtifacts
    runtimeLibs
    ;

  package = mkPackage commonArgs cargoArtifacts;

  # The same package from the `ci` profile, for `nix flake check`: it proves
  # the install steps work without a release build.
  ciPackage = mkPackage ciArgs ciArtifacts;
}
