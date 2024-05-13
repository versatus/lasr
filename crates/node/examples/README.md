# LASR's TiKV Persistence Store



## Prerequisites

1. Install [DockerEngine](https://docs.docker.com/engine/install/ubuntu/#install-using-the-repository).
2. Pull latest `pingcap` images for both `pd` and `tikv`:

    - `docker pull pingcap/pd:latest`
    - `docker pull pingcap/tikv:latest`

## Start TiKV Contrainer's

1. Create a script for each container to be built:
    - `pd-server.sh`:
        ```
        #!/bin/bash 
        
        docker run -d --name pd-server --network host pingcap/pd:latest \
                --name="pd1" \
                --data-dir="/pd1" \
                --client-urls="http://0.0.0.0:2379" \
                --peer-urls="http://0.0.0.0:2380" \
                --advertise-client-urls="http://0.0.0.0:2379" \
                --advertise-peer-urls="http://0.0.0.0:2380"
    - `tikv-server.sh`:
        ```
        #!/bin/bash

        docker run -d --name tikv-server --network host pingcap/tikv:latest \
                --addr="127.0.0.1:20160" \
                --advertise-addr="127.0.0.1:20160" \
                --data-dir="/tikv" \
                --pd="http://127.0.0.1:2379"
2. Run `pd-server.sh` **first**, followed by `tikv-server.sh`.

3. Ensure the container's were created by running `docker container list`.
    - Should look similar to this:

| CONTAINER ID | IMAGE                | COMMAND                   | CREATED          | STATUS         | PORTS | NAMES      |
|--------------|----------------------|---------------------------|------------------|----------------|-------|------------|
| 3120019ce8d5 | pingcap/tikv:latest | "/tikv-server --addr…"    | 9 seconds ago    | Up 8 seconds   |       | tikv-server|
| 026e259caf18 | pingcap/pd:latest   | "/pd-server --name=p…"    | 15 seconds ago   | Up 14 seconds  |       | pd-server  |

### Run example in LASR
`cargo run --package lasr_node --example tikv_example`

When the test passes, this ensures the TiKV persistence store for LASR was successful initiated! 
