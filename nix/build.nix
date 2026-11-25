# The Rust build, shared by the package, the checks and the dev shell.
#
# Crane builds the dependencies once (`cargoArtifacts`) and every Cargo
# check reuses them, so a change to our code never rebuilds the crates we
# borrow.
{ pkgs, crane }:
let
  craneLib = crane.mkLib pkgs;
  src = craneLib.cleanCargoSource ../.;
  cargoEnv = (pkgs.lib.importTOML ../.cargo/config.toml).env;
  appId = cargoEnv.NOVA_FILES_APP_ID;
  # xdg-desktop-portal finds the file picker by this bus name; portals.conf
  # picks it by the `.portal` file's name, its last part.
  portalBusName = cargoEnv.NOVA_FILES_PORTAL_BUS_NAME;
  portalName = pkgs.lib.last (pkgs.lib.splitString "." portalBusName);

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
    # The player helper opens libmpv at run time.
    mpv-unwrapped
  ];

  # Hashes of the git dependencies, so they are fetched as ordinary store
  # paths that the binary cache serves, not cloned at evaluation time on
  # every run (the zed fork's clone alone took a minute of each CI job).
  # After changing a git dependency, set its hash to `pkgs.lib.fakeHash`
  # and copy the real one from the build error.
  outputHashes = {
    "git+https://github.com/ImperaLuna/zed?rev=2033650502b1d0ec524b8913bf5a01b9463bed5d#2033650502b1d0ec524b8913bf5a01b9463bed5d" =
      "sha256-MAQ7KtgpBIJtnFntihhRTQrk+bvv0SCzDL8Q9ncb/0c=";
    "git+https://github.com/gfx-rs/wgpu?rev=cddb2f5d80dc0009b2ef6767635140cacf90a007#cddb2f5d80dc0009b2ef6767635140cacf90a007" =
      "sha256-OqPeK1cr3OSZgIl+F3GMuXWz31PxFoOO0u4INN9GmgM=";
    "git+https://github.com/zed-industries/font-kit?rev=94b0f28166665e8fd2f53ff6d268a14955c82269#94b0f28166665e8fd2f53ff6d268a14955c82269" =
      "sha256-KXygi0olNQi5yM8eaJVykNDtbPMDjT+cWPBF8UrtXR4=";
    "git+https://github.com/zed-industries/oo7?rev=731c1215edb6905876c6137ab300428c7b8d7c13#731c1215edb6905876c6137ab300428c7b8d7c13" =
      "sha256-QXblWsnq+graWOSW0iA8pUzpwizDLi/kOSWDdSQVZ/0=";
    "git+https://github.com/zed-industries/scap?rev=b5c86ec58dd4b6b7732bf6ac8be03d6b1f190a82#b5c86ec58dd4b6b7732bf6ac8be03d6b1f190a82" =
      "sha256-WVcjceVESUhPKL9z/1y+l/vmtHRJS4EecaMOaVN686o=";
    "git+https://github.com/zed-industries/wasm_thread?rev=0cf96c7708dfb97ccf3da50347e25edcf75d6937#0cf96c7708dfb97ccf3da50347e25edcf75d6937" =
      "sha256-+lRLCIk0S6Y5ORYjDKsYYHia2FtoSoh+rWkQh7mnPBE=";
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
    pname = (pkgs.lib.importTOML ../app/Cargo.toml).package.name;
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

  # The installed app: binaries, the D-Bus service files, the desktop entry
  # and its icon, the file picker's .portal file and the wrapper.
  mkPackage =
    args: artifacts:
    craneLib.buildPackage (
      args
      // {
        cargoArtifacts = artifacts;
        # Tests run once, in the nextest check, not again in the package.
        doCheck = false;
        # The same install script as `just install` (data/install.sh), so
        # the two layouts can't drift apart. Crane has already put the
        # programs in $out/bin.
        postInstall = ''
          bash ${../data}/install.sh --mode basic --prefix $out \
            --app-id ${appId} --portal-bus-name ${portalBusName}
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
    appId
    portalBusName
    portalName
    ;

  package = mkPackage commonArgs cargoArtifacts;

  # The same package from the `ci` profile, for `nix flake check`: it proves
  # the install steps work without a release build.
  ciPackage = mkPackage ciArgs ciArtifacts;
}
