# Versatus LASR

## Base Image

https://gvisor.dev/docs/user_guide/quick_start/oci/
https://hub.docker.com/_/busybox

docker export $(docker create busybox) | sudo tar -xf - -C rootfs --same-owner --same-permissions

https://taskfile.dev/