{
  description = ''
    A nix flake for development, and deployment of a LASR node.
  '';

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };

    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };

    versatus-nix = {
      url = "github:versatus/versatus.nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.fenix.follows = "fenix";
      inputs.flake-utils.follows = "flake-utils";
    };
  };

  outputs = { self, nixpkgs, crane, fenix, flake-utils, advisory-db, versatus-nix, ... }:
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;

          versaLib = versatus-nix.lib.${system};
          rustToolchain = versaLib.toolchains.mkRustToolchainFromTOML
            ./rust-toolchain.toml
            "sha256-SXRtAuO4IqNOQq+nLbrsDFbVk+3aVA8NNpSZsKlVH/8=";

          # Overrides the default crane rust-toolchain with fenix.
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain.fenix-pkgs;
          workspace = rec {
            # Inherit the workspace version from the node crate since the workspace is not a package.
            inherit (craneLib.crateNameFromCargoToml { cargoToml = (root + "/crates/node/Cargo.toml"); }) version;
            name = "lasr";
            root = ./.;
            src = craneLib.cleanCargoSource root;
          };

          # Common arguments can be set here to avoid repeating them later
          commonArgs = {
            inherit (workspace) version src;
            pname = workspace.name;
            strictDeps = true;

            # Inputs that must be available at the time of the build
            nativeBuildInputs = [ pkgs.pkg-config ];

            buildInputs = [
              pkgs.openssl.dev
              rustToolchain.darwin-pkgs
            ];
          };

          # Build *just* the cargo dependencies, so we can reuse
          # all of that work (e.g. via cachix) when running in CI
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          individualCrateArgs = commonArgs // {
            inherit cargoArtifacts;
            doCheck = false; # Use cargo-nextest below.
          };

          fileSetForCrate = crate: lib.fileset.toSource {
            root = workspace.root;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./crates
              (workspace.root + crate)
            ];
          };

          # Build the top-level crates of the workspace as individual derivations.
          # This allows consumers to only depend on (and build) only what they need.
          # Though it is possible to build the entire workspace as a single derivation,
          # in this case the workspace itself is not a package.
          mkCrateDrv = crate:
            let
              manifest = craneLib.crateNameFromCargoToml {
                cargoToml = (workspace.root + "${crate}/Cargo.toml");
              };
            in
            craneLib.buildPackage (individualCrateArgs // {
              inherit (manifest) version pname;
              cargoExtraArgs = "--locked --bin ${manifest.pname}";
              src = fileSetForCrate crate;
            });

          lasr_cli = mkCrateDrv "/crates/cli";
          lasr_node = mkCrateDrv "/crates/node";
        in
        {
          checks = {
            # Build the crate as part of `nix flake check` for convenience
            inherit lasr_cli lasr_node;

            # Run clippy (and deny all warnings) on the workspace source,
            # again, reusing the dependency artifacts from above.
            #
            # Note that this is done as a separate derivation so that
            # we can block the CI if there are issues here, but not
            # prevent downstream consumers from building our crate by itself.
            workspace-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              pname = workspace.name;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

            workspace-doc = craneLib.cargoDoc (commonArgs // {
              inherit cargoArtifacts;
              pname = workspace.name;
            });

            # Check formatting
            workspace-fmt = craneLib.cargoFmt {
              inherit (workspace) version src;
              pname = workspace.name;
            };

            # Audit dependencies
            workspace-audit = craneLib.cargoAudit {
              inherit (workspace) version src;
              inherit advisory-db;
              pname = workspace.name;
            };

            # Audit licenses
            workspace-deny = craneLib.cargoDeny {
              inherit (workspace) version src;
              pname = workspace.name;
            };

            # Run tests with cargo-nextest
            # Consider setting `doCheck = false` on other crate derivations
            # if you do not want the tests to run twice
            workspace-nextest = craneLib.cargoNextest (commonArgs // {
              inherit cargoArtifacts;
              pname = workspace.name;
              partitions = 1;
              partitionType = "count";
            });
          };

          packages =
            let
              hostPkgs = pkgs;
              guest_system = versaLib.virtualisation.mkGuestSystem pkgs;
              # Build packages for the linux variant of the host architecture, but preserve the host's
              # version of nixpkgs to build the virtual machine with. This way, building and running a
              # linux virtual environment works for all supported system architectures.
              lasrGuestVM = nixpkgs.lib.nixosSystem {
                system = null;
                modules = [
                  ./nixos/modules/deployments/common
                  ./nixos/modules/deployments/debug/debug-options.nix
                  versatus-nix.nixosModules.deployments.debugVm
                  ({
                    # Use the version of nixpkgs from the host to build the virtual image.
                    virtualisation.host.pkgs = hostPkgs;
                    nixpkgs.hostPlatform = guest_system;
                  })
                  ({
                    nixpkgs.overlays = [
                      self.overlays.rust-overlay
                      self.overlays.lasr-overlay
                    ];
                  })
                ];
              };
              # This would under normal circumstances be made available through `self.nixosConfigurations`
              # however, we want the darwin systems to build linux images since the `lasr_node` binary
              # has linux-only dependencies. Using the `nix-darwin` linux builder, MacOS users can still
              # build this image for deployment, and can use the `lasr-vm` for debugging.
              mkDigitalOceanImage = extraModules:
                nixpkgs.lib.nixosSystem {
                  system = guest_system;
                  modules = [
                    ./nixos/modules/deployments/common
                    versatus-nix.nixosModules.deployments.digitalOcean.digitalOceanImage
                    ({
                      nixpkgs.overlays = [
                        self.overlays.rust-overlay
                        self.overlays.lasr-overlay
                      ];
                    })
                  ] ++ extraModules;
                };
              debugDigitalOceanImage = mkDigitalOceanImage [
                ./nixos/modules/deployments/debug/debug-options.nix
              ];
            in
            {
              inherit lasr_cli lasr_node;

              lasr_debug_image =
                debugDigitalOceanImage.config.system.build.digitalOceanImage;

              # Spin up a virtual machine with the lasr_debug_image options
              # Useful for quickly debugging or testing changes locally
              lasr_vm = lasrGuestVM.config.system.build.vm;

              # TODO: Fix musl static linking
              # lasr_cli_cross = # this works on Linux only at the moment
              #   let
              #     archPrefix = builtins.elemAt (pkgs.lib.strings.split "-" system) 0;
              #     target = "${archPrefix}-unknown-linux-musl";

              #     staticCraneLib =
              #       let rustMuslToolchain = with fenix.packages.${system}; combine [
              #           minimal.cargo
              #           minimal.rustc
              #           targets.${target}.latest.rust-std
              #         ];
              #       in
              #       (crane.mkLib pkgs).overrideToolchain rustMuslToolchain;

              #     buildLasrCliStatic = { stdenv, pkg-config, openssl, libiconv, darwin }:
              #       staticCraneLib.buildPackage {
              #         pname = "lasr_cli";
              #         version = "1";
              #         src = lasrSrc;
              #         strictDeps = true;
              #         nativeBuildInputs = [ pkg-config ];
              #         buildInputs = [
              #           (openssl.override { static = true; })
              #           rustToolchain.darwin-pkgs
              #         ];

              #         doCheck = false;
              #         cargoExtraArgs = "--locked --bin lasr_cli";

              #         CARGO_BUILD_TARGET = target;
              #         CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
              #       };
              #   in
              #   pkgs.pkgsMusl.callPackage buildLasrCliStatic {}; # TODO: needs fix, pkgsMusl not available on darwin systems

              # TODO: Getting CC linker error
              # lasr_cli_windows =
              #   let
              #     crossPkgs = import nixpkgs {
              #       crossSystem = pkgs.lib.systems.examples.mingwW64;
              #       localSystem = system;
              #     };
              #     craneLib = 
              #       let 
              #         rustToolchain = with fenix.packages.${system}; combine [
              #             minimal.cargo
              #             minimal.rustc
              #             targets.x86_64-pc-windows-gnu.latest.rust-std
              #           ];
              #       in
              #       (crane.mkLib crossPkgs).overrideToolchain rustToolchain;

              #     inherit (crossPkgs.stdenv.targetPlatform.rust)
              #       cargoEnvVarTarget cargoShortTarget;

              #     buildLasrCli = { stdenv, pkg-config, openssl, libiconv, windows }:
              #       craneLib.buildPackage {
              #         pname = "lasr_node";
              #         version = "1";
              #         src = lasrSrc;
              #         strictDeps = true;
              #         nativeBuildInputs = [ pkg-config ];
              #         buildInputs = [
              #           (openssl.override { static = true; })
              #           windows.pthreads
              #         ];

              #         doCheck = false;
              #         cargoExtraArgs = "--locked --bin lasr_cli";

              #         CARGO_BUILD_TARGET = cargoShortTarget;
              #         CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
              #         "CARGO_TARGET_${cargoEnvVarTarget}_LINKER" = "${stdenv.cc.targetPrefix}cc";
              #         HOST_CC = "${stdenv.cc.nativePrefix}cc";
              #       };
              #   in
              #   crossPkgs.callPackage buildLasrCli {};
            } // lib.optionalAttrs (!pkgs.stdenv.isDarwin) {
              workspace-llvm-coverage = craneLib.cargoLlvmCov (commonArgs // {
                inherit cargoArtifacts;
              });
            };

          apps = {
            lasr_cli = flake-utils.lib.mkApp {
              drv = lasr_cli;
            };
            lasr_node = flake-utils.lib.mkApp {
              drv = lasr_node;
            };
          };

          devShells.default = craneLib.devShell {
            # Inherit inputs from checks.
            checks = self.checks.${system};

            # Extra inputs can be added here; cargo and rustc are provided by default.
            #
            # In addition, these packages and the `rustToolchain` are inherited from checks above:
            # cargo-audit
            # cargo-deny
            # cargo-nextest
            packages = with pkgs; [
              # ripgrep
              nil # nix lsp
              nixpkgs-fmt # nix formatter
            ];
          };

          formatter = pkgs.nixpkgs-fmt;
        }) // {
      overlays = {
        # Build lasr_cli & lasr_node so we can add their derivations to a configuration
        lasr-overlay = import ./nixos/overlay.nix;

        # This contains a lot of policy decisions which rust toolchain is used
        rust-overlay = final: prev: {
          craneLib = (self.inputs.crane.mkLib prev).overrideToolchain final.rustToolchain.fenix-pkgs;
          rustToolchain = self.inputs.versatus-nix.lib.${prev.system}.toolchains.mkRustToolchainFromTOML
            ./rust-toolchain.toml
            "sha256-SXRtAuO4IqNOQq+nLbrsDFbVk+3aVA8NNpSZsKlVH/8=";
        };
      };
    };
}
