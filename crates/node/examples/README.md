# LASR's TiKV Persistence Store



## Prerequisites

1. Install [DockerEngine](https://docs.docker.com/engine/install/ubuntu/#install-using-the-repository).
2. Pull latest `pingcap` images for both `pd` and `tikv`:

    - `docker pull pingcap/pd:latest`

    - `docker pull pingcap/tikv:latest`

## Start TiKV Contrainer's

TiKV requires at **minimum** 1 `pd` (Placement driver), and 1 `tikv` node, but are not limited.  
The Placement driver is the cluster manager of TiKV, and the TiKV node handles the `Store`'s.

Inside the root of the LASR repository, run the following commands **in order**:

1. Using `chmod +x <filename>`, we'll add execute permissions to the necessary scripts.
    - `chmod +x ./scripts/pd-server.sh`

    - `chmod +x ./scripts/tikv-server.sh`
2. Now we can execute the following scripts to build the required containers,   
 `pd-server.sh` **must** be executed **first**.

    - `./scripts/pd-server.sh`

    - `./scripts/tikv-server.sh` 
    

2. Ensure the container's were created by running `docker ps` in the terminal.
    - Should look similar to this:

| CONTAINER ID | IMAGE                | COMMAND                   | CREATED          | STATUS         | PORTS | NAMES      |
|--------------|----------------------|---------------------------|------------------|----------------|-------|------------|
| 3120019ce8d5 | pingcap/tikv:latest | "/tikv-server --addr…"    | 9 seconds ago    | Up 8 seconds   |       | tikv-server|
| 026e259caf18 | pingcap/pd:latest   | "/pd-server --name=p…"    | 15 seconds ago   | Up 14 seconds  |       | pd-server  |

### Run example in LASR
`cargo run --package lasr_node --example tikv_example`

When the test passes, this ensures the TiKV persistence store for LASR was successful initiated! 
