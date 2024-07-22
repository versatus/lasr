final: prev:

let
  craneLib = prev.craneLib;

  lasrArgs = {
    inherit (prev.workspace) src version;
    pname = prev.workspace.name;
    strictDeps = true;
    nativeBuildInputs = [ final.pkg-config ];
    buildInputs = [
      final.openssl.dev
      final.rustToolchain.darwin-pkgs
    ];
  };

  # Build *just* the cargo dependencies, so we can reuse
  # all of that work (e.g. via cachix) when running in CI
  lasrDeps = craneLib.buildDepsOnly lasrArgs;
in
{
  lasr_node = craneLib.buildPackage (lasrArgs // {
    pname = "lasr_node";
    doCheck = false;
    cargoArtifacts = lasrDeps;
    cargoExtraArgs = "--locked --bin lasr_node";
  });
  lasr_cli = craneLib.buildPackage (lasrArgs // {
    pname = "lasr_cli";
    doCheck = false;
    cargoArtifacts = lasrDeps;
    cargoExtraArgs = "--locked --bin lasr_cli";
  });
}
