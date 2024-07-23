# NixOS Deployments

## Deployment Processes

Deployment processes vary between server providers.

### DigitalOcean Deployment Process

1. NixOS images are created for each server
2. The images are uploaded to DigitalOcean via API, old images are deleted
3. The existing servers are rebuilt with the new images
4. `systemd` startup scripts run upon successful server start which spin up the node
5. This process continues automatically, pulling the most recent changes from the repository on a release cycle

> Note: It is now also possible to push a NixOS deployment to a server, without the DigitalOcean API via [nixos-anywhere](https://nix-community.github.io/nixos-anywhere/).
> Some configuration examples can be found at https://github.com/nix-community/nixos-anywhere-examples/.

## Local Development
It's possible to test your changes to a server locally by starting a NixOS virtual machine environment.

### NixOS VM Linux
Linux users have an easy time of this, and can simply `nix build .#<nixos-vm>`, then execute the resulting binary `./result/bin/<run-nixos-vm>`,
assuming they already have a nix installation.

### NixOS VM Darwin
Darwin users worry not, there is a fairly straight-forward solution with `nix-darwin`.

There are two resources we recommend for getting started with `nix-darwin` which should be followed in this order:
1. [nix-darwin setup](https://nixcademy.com/2024/01/15/nix-on-macos/#step-2-going-declarative-with-nix-darwin) 
2. [nix-darwin linux builder](https://nixcademy.com/2024/02/12/macos-linux-builder/#the-nix-darwin-option)

An example of a finalized `nix-darwin` flake, which enables the `linux-builder` functionality can be found at [nix-darwin-example](https://github.com/versatus/versatus.nix/tree/master/nix-darwin).
From this point, the steps for running the virtual environment are [the same as Linux](#nixos-vm-linux)! 🎉
```sh
nix build .#<nixos-vm>
./result/bin/<run-nixos-vm>
```

## Rebuilding The NixOS Server
**Be aware that changes to the server with these methods are semi-permanent at best. To add permanent changes please file
an issue and pull request that closes said issue.**

### Before Rebuilding...
Be aware that unless the packages need to be made available semi-permanently on the server,
using the `nix-shell` feature will open a shell with the packages added to `$PATH`
and is often the solution if a developer tool is needed only temporarily, or for testing means **while on the server**.
> For more commands please consult the manual: `nix-shell --help`.

```sh
ssh <user>@<ip-address>
# make packages available on the current $PATH (exiting the shell will remove them)
nix-shell -p <package-name1> <package-name2>
# or if just needing to run a program once
nix-shell --run "command arg1 arg2 ..."
```

In the case of needing to update the configuration on a development server where you may be testing new features, etc., 
there are two main ways to apply your changes, but both rely on the `nixos-rebuild switch` command with the option `--flake`.

The first instinct for seasoned NixOS users would be to edit, and rebuild as if it was a local system, however this ins't
the correct way to go about it when dealing with NixOS servers. I've tentatively added instructions on how to properly go
about rebuilding the system in the `configuration.nix` file, which (on the server) can be found at `/etc/nixos/configuration.nix`,
the contents of which are more or less as follows:
> Note that attempting to rebuild switch _will fail_ since the `configuration.nix` file does not have anything in it except these instructions.

### Rebuild From Your Local Machine
This will be the most common way of adding **semi-permanent changes**, since it doesn't require the server
itself to be aware of the original configuration, i.e. the versatus.nix git repository.
Likely the changes that will often be made are to the packages included on the server,
and those packages should be added under `environment.systemPackages` in `deployments/<name-of-image>/common/default.nix`.
This command is especially helpful when needing to test changes that will eventually be applied permanently
to the server image documented in the [Deployment Processes](#deployment-processes).

To rebuild the configuration on a server please update your local copy of
the configuration you are trying to change in the nix flake, then target the server
you would like to rebuild with that configuration from your machine.

```sh
nixos-rebuild \
  --flake .#<name-of-configuration> \
  --build-host <user>@<ip-address> \
  --target-host <user>@<ip-address> \
  switch
```

Additionally, MacOS users must pass the `--fast` flag.
The build host and target host will be the same, and if the user is not `root`, you must
also pass it the `--use-remote-sudo` flag, assuming the user has sudo privileges.

### Rebuild From The Server
If rebuilding from the server itself, you must have a copy of the original flake used
to produce the system configuration you are on.

```sh
ssh <user>@<ip-address>
git clone <flake-repository>
```

Make your changes and be sure to add new files to git, otherwise the flake will not apply them.
Then rebuild from the flake:

```sh
git add -A
nixos-rebuild switch --flake .#<name-of-configuration>
```

## Troubleshooting

### Unable To Connect After Server Or VM Rebuild
After rebuilding the server with `nixos-rebuild`, or the server is rebuilt from a newer
version of the NixOS image, you may encounter an error when attempting to connect to the
server via SSH. Multiple attempts of this will prompt you with a warning of potential
"man-in-the-middle" attacks. To reset your relationship with the server navigate to your
`~/.ssh` folder, and remove the server's host keys from the `known_hosts` file. You should
be able to now connect to the server, which will prompt you to add the new keys to `~/.ssh/known_hosts`. 
The same can happen when developing on local NixOS VMs. The solution is the same, but in
addition the `.qcow2` file that is produced after loading the image can be removed if
system state does not need to be restored from the previous boot.
