{
  description = "lez-multisig-framework — LEZ Multisig governance library + C FFI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, rust-overlay, ... }:
    let
      lib = nixpkgs.lib;

      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = lib.genAttrs systems;

      mkPkgs = system: import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
      };
    in
    {
      packages = forAll (system:
        let
          pkgs = mkPkgs system;

          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          # Fetch pre-built circuit files from official logos-blockchain-circuits releases
          circuitsVersion = "v0.4.1";
          circuitsPlatform = {
            "x86_64-linux"   = "linux-x86_64";
            "aarch64-linux"  = "linux-aarch64";
            "x86_64-darwin"  = "macos-x86_64";
            "aarch64-darwin" = "macos-aarch64";
          }.${system};

          logosBlockchainCircuits = pkgs.fetchurl {
            url = "https://github.com/logos-blockchain/logos-blockchain-circuits/releases/download/${circuitsVersion}/logos-blockchain-circuits-${circuitsVersion}-${circuitsPlatform}.tar.gz";
            hash = {
              "x86_64-linux"   = "sha256-Oi3xhqm5Sd4PaCSHWMvsJm2YPtSlm11BBG99xG30tiM=";
              "aarch64-linux"  = "";
              "x86_64-darwin"  = "";
              "aarch64-darwin" = "";
            }.${system};
          };

          circuitsDir = pkgs.runCommand "logos-blockchain-circuits" {} ''
            mkdir -p $out
            tar xzf ${logosBlockchainCircuits} -C $out --strip-components=1
          '';

          # Pre-built NSSA program method binaries (needed by nssa build.rs)
          nssaProgramMethods = pkgs.fetchurl {
            url = "https://github.com/jimmy-claw/spelbook/releases/download/circuits-v0.1.0/nssa-program-methods.tar.gz";
            sha256 = "a40ee19678cb44b07167dbe7ccc3e7279585d7fb6182831d792c03e6ad2b64d5";
          };

          artifactsDir = pkgs.runCommand "nssa-artifacts" {} ''
            mkdir -p $out/program_methods
            tar xzf ${nssaProgramMethods} -C $out
          '';

          # Filter workspace root — include Rust/Cargo files and the FFI include/ dir
          src = lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || (lib.hasInfix "/include/" path)
              || (lib.hasSuffix ".h" path)
              || (lib.hasSuffix ".json" path);
          };

          commonArgs = {
            inherit src;
            pname = "lez-multisig-ffi";
            version = "0.1.0";

            # Build only the FFI crate
            cargoExtraArgs = "-p lez-multisig-ffi";

            nativeBuildInputs = with pkgs; [
              pkg-config
              protobuf
            ];

            buildInputs = with pkgs; [
              openssl
            ] ++ lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];

            # risc0 guest builds need this
            RISC0_SKIP_BUILD = "1";

            # logos-blockchain-pol build.rs needs circuits directory
            LOGOS_BLOCKCHAIN_CIRCUITS = "${circuitsDir}";

            preBuild = ''
              echo "=== Injecting NSSA artifacts ==="
              VENDOR_DIR=$(grep 'directory = ' .cargo-home/config.toml .cargo/config.toml 2>/dev/null | head -1 | sed 's/.*directory = "//;s/"//' || true)
              if [ -z "$VENDOR_DIR" ]; then
                echo "WARNING: Could not find vendor dir in cargo config, searching..."
                VENDOR_DIR=$(find /nix/store -maxdepth 1 -name '*vendor-cargo-deps' -type d 2>/dev/null | head -1 || true)
              fi
              echo "Vendor dir: $VENDOR_DIR"

              VENDOR_BASE=$(dirname "$VENDOR_DIR")
              echo "Vendor base: $VENDOR_BASE"

              if [ -n "$VENDOR_BASE" ]; then
                NSSA_DIR=$(find -L "$VENDOR_BASE" -maxdepth 4 -name 'nssa-0*' -type d 2>/dev/null | head -1 || true)
                if [ -n "$NSSA_DIR" ]; then
                  PARENT=$(dirname "$NSSA_DIR")
                  echo "Found nssa at: $NSSA_DIR"

                  WRITABLE_VENDOR="$PWD/vendor-writable"
                  echo "Creating writable vendor copy at $WRITABLE_VENDOR..."
                  cp -rL "$VENDOR_BASE" "$WRITABLE_VENDOR"
                  chmod -R u+w "$WRITABLE_VENDOR"

                  NSSA_DIR2=$(find "$WRITABLE_VENDOR" -maxdepth 4 -name 'nssa-0*' -type d | head -1)
                  PARENT2=$(dirname "$NSSA_DIR2")
                  mkdir -p "$PARENT2/artifacts/program_methods"
                  cp ${artifactsDir}/program_methods/*.bin "$PARENT2/artifacts/program_methods/"
                  echo "Injected artifacts at $PARENT2/artifacts/"
                  ls -la "$PARENT2/artifacts/program_methods/"

                  for cfg in .cargo-home/config.toml .cargo/config.toml; do
                    if [ -f "$cfg" ]; then
                      sed -i "s|$VENDOR_BASE|$WRITABLE_VENDOR|g" "$cfg"
                      echo "Updated $cfg"
                    fi
                  done
                else
                  echo "WARNING: nssa crate not found in $VENDOR_BASE"
                fi
              else
                echo "WARNING: No vendor dir found"
              fi
              echo "=== Done injecting NSSA artifacts ==="
            '';
          };

          # Build deps first (for caching)
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          # Build the actual FFI crate
          lezMultisigFfi = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;

            postInstall = ''
              mkdir -p $out/lib $out/include

              find target -name "liblez_multisig_ffi.so" -o -name "liblez_multisig_ffi.dylib" | head -1 | while read f; do
                cp "$f" $out/lib/
              done

              find target -name "liblez_multisig_ffi.a" | head -1 | while read f; do
                cp "$f" $out/lib/
              done

              cp lez-multisig-ffi/include/*.h $out/include/ 2>/dev/null || true
            '';
          });
        in
        {
          default = lezMultisigFfi;
          lib = lezMultisigFfi;
        }
      );

      devShells = forAll (system:
        let
          pkgs = mkPkgs system;
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];
            packages = with pkgs; [ rust-analyzer ];
          };
        }
      );
    };
}
