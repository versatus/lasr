# lasr_cli

Command line interface for interacting with a LASR node.

## Prerequisites

### System

Unlike its node counterpart, the node CLI is meant to work on any system. A work-in-progress statically linked
binary is available through the Nix flake at the root of the project, and development is on-going.
Non-statically linked binaries are already available for the following systems:

- x86_64-linux
- x86_64-darwin
- aarch64-linux
- aarch64-darwin

### Environment Variables

| Environment Variable   | Description                                                               |
|------------------------|---------------------------------------------------------------------------|
| `LASR_RPC_URL`         | URL used for remote procedure calls, defaults to `http://127.0.0.1:9292`. |
