# LASR Node Digital Ocean Deployment Process

This details the entire process of deploying a LASR Node from the perspective of a completely
new machine on Digital Ocean, zero to deployed. This process is hand-rolled by the Versatus
team and is the tested route for deploying a LASR Node.

Some important directories to note before getting started:
- `/app` - this is the working directory
  - `/app/base_image` - contains all of the `gVisor` runtimes
  - `/app/bin` - contains all of the setup scripts
  - `/app/eigenda` - a git clone of the EigenDA repository
  - `/app/lasr` - a git clone of the LASR Node repository (if using a pre-built binary this may appear as `lasr_node`)
- `~/kubo` - the IPFS daemon

## Getting Started

1. Open an inbound port from the Digital Ocean site. This is the port that the LASR Node RPC server will use.
  > Note: Assign the port to the `PORT` environment variable documented in the root directory README.

2. Create the working directory `mkdir /app` on the DO Box.

3. Install the dependencies listed under [Dependencies](#Dependencies) to the DO Box.
  > Important: Be aware that while certain dependencies _could_ be installed globally, eg Git, others
  must be available in the `/app` or `$HOME` directory in order for the pre-written scripts to work.

4. Clone the `eigenda` & `lasr` git repositories into the `/app` directory.
  ```sh
  git clone https://github.com/Layr-Labs/eigenda.git
  git clone https://github.com/versatus/lasr.git
  ```

5. Execute the setup scripts, located in `/app/bin`, in this order:
  > Note: These scripts **must** be executed from the `/app` directory in order to exit successfully.
  If a script does not execute, you may need to make it executable: `chmod +x <script>`.
  ```sh
  
  ```

## Dependencies

These dependencies..

#### Global

..can be installed globally.

- Docker
  > Note: Collisions between Docker Desktop and Docker daemon have been observed, thus it is
  advised to only install the Docker dependencies directly and _not_ the Docker Desktop app.
- Git
- Rust
- Overmind (tmux based process manager)

#### Working Directory

..must be installed in the working directory, `/app`.

- gVisor (OCI compatible container, `runsc`)
  > Note that currently only `x86_64-linux` is verified to work with `gVisor`.
- gRPCurl (CLI tool for interacting with gRPC servers)

#### Home

..must be installed in the `$HOME` directory.

- Kubo (IPFS daemon)
