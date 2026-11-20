{
  description = "Desktop app that edits Star Citizen settings, keybinds, StarStrings, screenshots and channels";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forEachSystem (pkgs: rec {
        star-config = pkgs.callPackage ./nix/package.nix { };
        default = star-config;
      });

      overlays.default = final: _prev: {
        star-config = final.callPackage ./nix/package.nix { };
      };

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.star-config ];
          packages = [
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
          ];
          # `cargo run` has no patched rpath, so the libraries loaded with dlopen at run
          # time come from here.
          LD_LIBRARY_PATH =
            pkgs.lib.makeLibraryPath
              self.packages.${pkgs.stdenv.hostPlatform.system}.star-config.runtimeLibs;
          ICED_TEST_BACKEND = "tiny-skia";
        };
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt);
    };
}
