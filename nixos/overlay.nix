final: prev:

let
  inherit (prev) lib craneLib;
  workspace = rec {
    inherit (craneLib.crateNameFromCargoToml { cargoToml = (root + "/crates/node/Cargo.toml"); }) version;
    name = "lasr";
    root = ../.;
    src = craneLib.cleanCargoSource root;
  };

  commonArgs = {
    inherit (workspace) src version;
    pname = workspace.name;
    strictDeps = true;
    nativeBuildInputs = [ final.pkg-config ];
    buildInputs = [
      final.openssl.dev
      final.rustToolchain.darwin-pkgs
    ];
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
  individualCrateArgs = commonArgs // {
    inherit cargoArtifacts;
    doCheck = false;
  };

  fileSetForCrate = crate: lib.fileset.toSource {
    root = workspace.root;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      (workspace.root + crate)
    ];
  };

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
in
{
  lasr_cli = mkCrateDrv "/crates/cli";
  lasr_node = mkCrateDrv "/crates/node";
}
