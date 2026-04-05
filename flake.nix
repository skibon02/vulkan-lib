{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
        config.android_sdk.accept_license = true;
        config.allowUnfree = true;
      };
      # Simplified nightly toolchain
      rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
        extensions = [ "rust-src" "rust-analyzer" ];
        targets = [ "aarch64-linux-android" "x86_64-linux-android" ];
      };
      androidSdk = (pkgs.androidenv.composeAndroidPackages {
        buildToolsVersions = [ "34.0.0" ];
        platformVersions = [ "30" "33" "34" ];
        abiVersions = [ "arm64-v8a" "x86_64" ];
        includeNDK = true;
        ndkVersions = [ "26.1.10909125" ];
        cmakeVersions = [ "3.22.1" ];
      }).androidsdk;
    in {
      devShells.${system}.default = pkgs.mkShell {
        buildInputs = with pkgs; [
          rustToolchain
          pkg-config
          androidSdk
          jdk17

            cargo-apk
            androidenv.androidPkgs.platform-tools
        ];

        RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
        ANDROID_NDK_ROOT = "${androidSdk}/libexec/android-sdk/ndk/26.1.10909125";

        shellHook = ''
          # Create a stable symlink for IDEs
          ln -sfn ${rustToolchain}/bin .toolchain
        '';

        LD_LIBRARY_PATH = with pkgs; lib.makeLibraryPath [
          libGL
          libxkbcommon
          wayland
          vulkan-loader
        ];
      };
    };
}
