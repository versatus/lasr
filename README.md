# Versatus LASR

Language Agnostic Stateless Rollup.

## LASR Node Environment Variables

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

## LASR CLI Environment Variables

| Environment Variable   | Description                                                               |
|------------------------|---------------------------------------------------------------------------|
| `LASR_RPC_URL`         | URL used for remote procedure calls, defaults to `http://127.0.0.1:9292`. |

## Base Image

```sh
docker export $(docker create busybox) | sudo tar -xf - -C rootfs --same-owner --same-permissions
```

Relevant documentation for the oci-runtime, busybox and taskfile:
- https://gvisor.dev/docs/user_guide/quick_start/oci/
- https://hub.docker.com/_/busybox
- https://taskfile.dev/

### Handy commands

Register a program.
```sh
cli wallet register-program --from-file --inputs '{"contentId": "0x742d35cc6634c0532925a3b844bc454e4438f44e"}'
```

Call a program method.
```sh
cli wallet call --from-file --to 0x742d35cc6634c0532925a3b844bc454e4438f44e -c 0x742d35cc6634c0532925a3b844bc454e4438f44e --op getName --inputs '{"first_name": "Andrew", "last_name": "Smith"}' -v 0
```

Test gVistor runtime.
```sh
runsc --debug --debug-log=/tmp/runsc/ --TESTONLY-unsafe-nonroot do echo 123
runsc --debug --debug-log=/tmp/runsc/ do echo 123
```
