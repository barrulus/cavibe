{
  description = "Cavibe - Audio visualizer with animated song display";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain
            rustToolchain
            cargo-watch
            cargo-edit

            # Audio libraries
            alsa-lib
            alsa-plugins
            pipewire
            pulseaudio

            # Build dependencies
            pkg-config

            # For potential GUI/wallpaper mode
            wayland
            libxkbcommon
            xorg.libX11
            xorg.libXrandr
            xorg.libXinerama
            xorg.libXcursor
            xorg.libXi

            # D-Bus for MPRIS metadata
            dbus
          ];

          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
              pkgs.alsa-lib
              pkgs.alsa-plugins
              pkgs.pulseaudio
              pkgs.pipewire
              pkgs.wayland
              pkgs.libxkbcommon
              pkgs.xorg.libX11
              pkgs.xorg.libXrandr
              pkgs.xorg.libXinerama
              pkgs.xorg.libXcursor
              pkgs.xorg.libXi
              pkgs.dbus
            ]}:$LD_LIBRARY_PATH"
            export PKG_CONFIG_PATH="${pkgs.alsa-lib.dev}/lib/pkgconfig:${pkgs.dbus.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
            # Point ALSA to PipeWire plugins for loopback support
            export ALSA_PLUGIN_DIR="${pkgs.alsa-plugins}/lib/alsa-lib"
            echo "Cavibe development environment loaded"
            echo "Run 'cargo build' to build the project"
            echo "Run 'cargo run' to start the visualizer"
          '';
        };

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "cavibe";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [
            alsa-lib
            pulseaudio
            dbus
          ];
        };
      });
}
