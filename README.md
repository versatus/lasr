# Versatus LASR

## LASR Node Environment Variables

SECRET_KEY
BLOCKS_PROCESSED_PATH
ETH_RPC_URL

## CLI Environment Variables

LASR_RPC_URL

## Base Image

https://gvisor.dev/docs/user_guide/quick_start/oci/
https://hub.docker.com/_/busybox

docker export $(docker create busybox) | sudo tar -xf - -C rootfs --same-owner --same-permissions

https://taskfile.dev/

### Handy commands 

`cli wallet register-program --from-file --inputs '{"contentId": "0x742d35cc6634c0532925a3b844bc454e4438f44e"}'`

`cli wallet call --from-file --to 0x742d35cc6634c0532925a3b844bc454e4438f44e -c 0x742d35cc6634c0532925a3b844bc454e4438f44e --op getName --inputs '{"first_name": "Andrew", "last_name": "Smith"}' -v 0`

`runsc --debug --debug-log=/tmp/runsc/ --TESTONLY-unsafe-nonroot do echo 123`

`runsc --debug --debug-log=/tmp/runsc/ do echo 123`