# Versatus LASR

Language Agnostic Stateless Rollup.

## Usage

All of our documentation including design details and deploying programs can be found on our documentation site: https://docs.versatus.io/

## Getting Started

The fastest way to see a LASR node in action is by using our internal Nix tooling at https://github.com/versatus/versatus.nix
which includes a local NixOS VM that will automatically spin up a LASR node and works with Linux/Darwin systems as well as WSL.
Generic modules are also available there, making it seamless to create a custom image for your personal node server. A production
configuration is also available for deploying to DigitalOcean servers.

## Environment Variables

### LASR Node Environment Variables

| Environment Variable    | Description                                                      |
|-------------------------|------------------------------------------------------------------|
| `SECRET_KEY`            | Used for signing transactions and securing connections.          |
| `BLOCKS_PROCESSED_PATH` | Path where processed blocks information is stored.               |
| `ETH_RPC_URL`           | URL for Ethereum RPC endpoint.                                   |
| `EO_CONTRACT_ADDRESS`   | Address of the Executable Oracle contract.                       |
| `COMPUTE_RPC_URL`       | URL for the compute RPC endpoint.                                |
| `STORAGE_RPC_URL`       | URL for the compute RPC endpoint.                                |
| `PORT`                  | Optionally specify a port, defaults to `9292`.                   |
| `BATCH_INTERVAL`        | Interval in secs that transactions are batched, defaults to 180. |
| `VIPFS_ADDRESS`         | Optional. Used by the OciManager.                                |

### LASR CLI Environment Variables

| Environment Variable   | Description                                                               |
|------------------------|---------------------------------------------------------------------------|
| `LASR_RPC_URL`         | URL used for remote procedure calls, defaults to `http://127.0.0.1:9292`. |

## Contributing Guide

Contributors are welcome, and we encourage interacting with us on Discord where we can help guide and
answer any questions.

### Fork And Clone The Repository
Create your own fork of the `lasr` repository, and create a local copy of your fork.
```
git clone github:<your-username>/lasr
```

### Build The Project
LASR is built with Rust, so a Rust toolchain is a pre-requisite. If you have `rustup` installed
the project's `rust-toolchain.toml` will enable the current toolchain our core team uses.
```
cd lasr && cargo build
```

### Testing Your Changes
To see your changes take effect, we suggest adding an appropriate unit or integration test, however
some testing cases (such as bug fixes) may require manual testing, in which case using the [`versatus.nix` flake](https://github.com/versatus/versatus.nix)
is the easiest option. After following the setup steps there, testing your changes is straight-forward:

- unit or integration tests with `cargo`
```sh
# The mock_storage feature enables an in-memory variant of node storage
cargo test --workspace --features mock_storage
```

- `versatus.nix` manual debugging

> Note: To see your changes take effect, you will need to replace `inputs.lasr.url` in the `flake.nix`
> with the URL to your upstream branch.

```sh
nix flake update # updates flake inputs, see note above
nix build .#lasr_vm
./result/bin/run-lasr-nightly-server-vm
```

> Note: A qemu terminal should appear which you can either login as root, or ssh into.
> A `logs` folder should appear after a minute or so, with node debug info. Further info
> aboute the running node service can be found with `journalctl -b | grep node-start`.
