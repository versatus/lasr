# Versatus LASR

Language Agnostic Stateless Rollup.

## Usage

There are two main binaries that result from this repository:
- [lasr_cli](./crates/cli): Command line interface for the `lasr_node` binary.
- [lasr_node](./crates/node): The LASR node itself.

Further documentation including design details and instructions for deploying programs for LASR can be found on our documentation site: https://docs.versatus.io/

## Getting Started

The fastest way to see a LASR node in action is by using our internal Nix tooling at the root of this project
which includes a local NixOS VM that will automatically spin up a LASR node and works with Linux/Darwin systems, as well as WSL.
A GitHub codespaces devcontainer is also available for those who would rather not install Nix as a dependency.

A guide for installing Nix can be found here: https://github.com/versatus/versatus.nix.

## Contributing Guide

Contributors are welcome, and we encourage interacting with us on Discord where we can help guide and
answer any questions.

### Fork And Clone The Repository
Create your own fork of the `lasr` repository, and create a local copy of your fork.
```sh
git clone github:<your-username>/lasr
```

### Build The Project
LASR is built with Rust, so a Rust toolchain is a pre-requisite. If you have `rustup` installed
the project's `rust-toolchain.toml` will enable the current toolchain our core team uses.

```sh
cd lasr && cargo build
```

Nix users can start an interactive development shell with `nix develop`, which provides the Rust toolchain automatically.

```sh
cd lasr && nix develop
cargo build
```

### Testing Your Changes
To see your changes take effect, we suggest adding an appropriate unit or integration test, however
some testing cases (such as bug fixes) may require manual testing, in which case using the project's nix flake
is the easiest option. See [Getting Started](#getting-started) for setup steps, then testing your changes is straight-forward:

- unit or integration tests with `cargo`
```sh
# The mock_storage feature enables an in-memory variant of node storage
cargo test --workspace --features mock_storage
```

- NixOS VM manual debugging

```sh
nix build .#lasr_vm
./result/bin/run-lasr-debug-server-vm
```

> Note: A qemu terminal should appear which you can either login as root, or ssh into.
> A `logs` folder should appear after a minute or so, with node debug info. Further info
> aboute the running node service can be found with `journalctl -b | grep node-start`.

### Code Review
Pull requests must meet the following requirements:
- A corresponding issue and closing keywords for that issue in the pull request description
- A description of how your solution solves the issue
- Tests that prove the effectiveness of your changes (if applicable)
- Branch must be up-to-date with `main`
- All status checks must pass

A core contributor will be assigned to review your pull request in 24-48hrs.
Please be sure the above guidelines are followed, failure to meet them could result in your PR being closed!
If the issue you are having is not listed, feel free to open one. Doing so helps us point you in the right
direction, which could save you precious hours. Please understand that feature requests that are not on the
roadmap may not be approved, but don't let it deter you from suggesting features you'd like to see implemented.
Happy hacking!
