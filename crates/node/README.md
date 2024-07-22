# lasr_node

The LASR node binary.

## Prerequisites

### System

The `lasr_node` binary relies heavily on some Linux-only dependencies, listed under [System Dependencies](#system-dependencies) below.
Therefore, it requires Linux, meaning either your system must have Linux installed or be capable of running a Linux container.
This is confirmed working with Nix for the following targets, but may also be possible on others such as WSL:

- x86_64-linux
- x86_64-darwin
- aarch64-linux
- aarch64-darwin

> Note: MacOS must use the `nix-darwin` linux builder feature in order to work.
> See https://github.com/versatus/versatus.nix/blob/master/README.md for details.

### System Dependencies

Listed are dependencies that must be present on your system for the LASR node to run.

- gVisor (`runsc`): At the time of writing, only available for AMD64 and ARM64 Linux.
- IPFS
- grpcurl

### Environment Variables

An [example configuration](./.env-sample) can be found at the root of _this crate_.

On some systems, the default path and command for `runsc` and `grpcurl` may fail. In such cases,
setting their respective binary path variables may help.

| Environment Variable     | Description                                                      |
|--------------------------|------------------------------------------------------------------|
| `SECRET_KEY`             | Used for signing transactions and securing connections.          |
| `BLOCKS_PROCESSED_PATH`  | Path where processed blocks information is stored.               |
| `ETH_RPC_URL`            | URL for Ethereum RPC endpoint.                                   |
| `EO_CONTRACT_ADDRESS`    | Address of the Executable Oracle contract.                       |
| `COMPUTE_RPC_URL`        | URL for the compute RPC endpoint.                                |
| `STORAGE_RPC_URL`        | URL for the compute RPC endpoint.                                |
| `PORT`                   | Optionally specify a port, defaults to `9292`.                   |
| `BATCH_INTERVAL`         | Interval in secs that transactions are batched, defaults to 180. |
| `VIPFS_ADDRESS`          | Optional. Used by the OciManager.                                |
| `RUNSC_BIN_PATH`         | Optionally specify the path to the gVisor runsc binary.          |
| `GRPCURL_BIN_PATH`       | Optionally specify the path to the grpcurl binary.               |
| `EIGENDA_SERVER_ADDRESS` | Optional. Defaults to `disperser-holesky.eigenda.xyz:443`.       |
