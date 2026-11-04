{
  lib,
  rustPlatform,
  pkg-config,
  cmake,
  makeWrapper,
  fontconfig,
  freetype,
  libxkbcommon,
  wayland,
  vulkan-loader,
  libGL,
  libx11,
  libxcb,
  libxcursor,
  libxi,
  libxrandr,
  zstd,
}:

let
  # GPUI opens these with dlopen at run time, so they go on the library path
  # of the wrapped binary, not only on the link line.
  runtimeLibs = [
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
in
rustPlatform.buildRustPackage {
  pname = "nova-files";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    pkg-config
    cmake
    makeWrapper
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    fontconfig
    freetype
    libxkbcommon
    wayland
    libx11
    libxcb
    zstd
  ];

  postFixup = ''
    wrapProgram $out/bin/nova-files \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
  '';

  passthru = { inherit runtimeLibs; };

  meta = {
    description = "A fast, rounded file manager for Linux";
    homepage = "https://github.com/ImperaLuna/Nova-Files";
    license = lib.licenses.mit;
    mainProgram = "nova-files";
    platforms = lib.platforms.linux;
  };
}
