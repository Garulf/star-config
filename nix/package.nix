{
  lib,
  rustPlatform,
  copyDesktopItems,
  makeBinaryWrapper,
  makeDesktopItem,
  dbus,
  libGL,
  libx11,
  libxcursor,
  libxi,
  libxkbcommon,
  libxrandr,
  vulkan-loader,
  wayland,
  xdg-utils,
}:

let
  manifest = lib.importTOML ../Cargo.toml;

  # winit, wgpu and rfd's portal dialog load these with dlopen, so the linker never
  # records them.
  runtimeLibs = [
    dbus
    libGL
    libx11
    libxcursor
    libxi
    libxkbcommon
    libxrandr
    vulkan-loader
    wayland
  ];
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "star-config";
  inherit (manifest.workspace.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../LICENSE
      ../crates
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package"
    "sc-app"
  ];

  # The suite renders every screen and takes minutes. CI runs it on each push.
  doCheck = false;

  nativeBuildInputs = [
    copyDesktopItems
    makeBinaryWrapper
  ];

  desktopItems = [
    (makeDesktopItem {
      name = "star-config";
      desktopName = "star-config";
      comment = finalAttrs.meta.description;
      exec = finalAttrs.meta.mainProgram;
      icon = "input-gaming";
      terminal = false;
      categories = [
        "Game"
        "Utility"
      ];
      keywords = [
        "Star Citizen"
        "keybinds"
        "user.cfg"
      ];
      startupWMClass = "star-config";
    })
  ];

  # The libraries go into the rpath, not LD_LIBRARY_PATH, because the app starts the
  # launcher and Wine, and an exported library path would leak into them.
  postFixup = ''
    patchelf --add-rpath "${lib.makeLibraryPath runtimeLibs}" $out/bin/star-config
    wrapProgram $out/bin/star-config \
      --suffix PATH : "${lib.makeBinPath [ xdg-utils ]}"
  '';

  passthru = { inherit runtimeLibs; };

  meta = {
    description = "Edit Star Citizen settings, keybinds, StarStrings, screenshots and channels";
    homepage = "https://github.com/Garulf/star-config";
    license = lib.licenses.mit;
    mainProgram = "star-config";
    platforms = lib.platforms.linux;
  };
})
