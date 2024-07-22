{ pkgs, ... }:
let
  system = pkgs.stdenv.hostPlatform.system;
  # Pull the PD server image from dockerhub
  pd-image =
    let
      name = "pingcap/pd";
      platformSha256 = {
        "aarch64-linux" = "sha256-+IBB5p1M8g3fLjHbF90vSSAoKUidl5cdkpTulkzlMAc=";
        "x86_64-linux" = "sha256-xNPJrv8y6vjAPNvn9lAkghCfRGUDiBfRCUBsEYvb49Q=";
      }."${system}" or (builtins.throw "Unsupported platform for docker image ${name}, must either be arm64 or amd64 Linux: found ${system}");
    in
    pkgs.dockerTools.pullImage {
      imageName = name;
      imageDigest = "sha256:0e87d077d0fd92903e26a6ebeda633d6979380aac6fc76aa24c6a02d25a404f6";
      sha256 = platformSha256;
      finalImageTag = "latest";
      finalImageName = name;
    };
  # Starts the placement driver server for TiKV.
  # NOTE: This is a global script, which is run by default and is only
  # necessary in scenarios where the server does not start automatically.
  start-pd-server = pkgs.writeShellScriptBin "start-pd-server.sh" ''
    docker run -d --name pd-server --network host pingcap/pd:latest \
        --name="pd1" \
        --data-dir="/pd1" \
        --client-urls="http://0.0.0.0:2379" \
        --peer-urls="http://0.0.0.0:2380" \
        --advertise-client-urls="http://0.0.0.0:2379" \
        --advertise-peer-urls="http://0.0.0.0:2380"
  '';
  # Pull the TiKV server image from dockerhub
  tikv-image =
    let
      name = "pingcap/tikv";
      platformSha256 = {
        "aarch64-linux" = "sha256-JbogHq9FLfm7x08xkwiDF0+YyUKRXF34vHty+ZxIZh0=";
        "x86_64-linux" = "sha256-udLF3mAuUU08QX2Tg/mma9uu0JdtdJuxK3R1bqdKjKk=";
      }.${system} or (builtins.throw "Unsupported platform for docker image ${name}, must either be arm64 or amd64 Linux: found ${system}");
    in
    pkgs.dockerTools.pullImage {
      imageName = name;
      imageDigest = "sha256:e68889611930cc054acae5a46bee862c4078af246313b414c1e6c4671dceca63";
      sha256 = platformSha256;
      finalImageTag = "latest";
      finalImageName = name;
    };
  # Starts the TiKV server.
  # NOTE: This is a global script, which is run by default and is only
  # necessary in scenarios where the server does not start automatically.
  start-tikv-server = pkgs.writeShellScriptBin "start-tikv-server.sh" ''
    docker run -d --name tikv-server --network host pingcap/tikv:latest \
        --addr="127.0.0.1:20160" \
        --advertise-addr="127.0.0.1:20160" \
        --data-dir="/tikv" \
        --pd="http://127.0.0.1:2379"
  '';
  busybox-stream =
    let
      name = "busybox";
      tag = "latest";
      platformSha256 = {
        "aarch64-linux" = "sha256-Oq9xmrdoAJvwQl9WOBkJFhacWHT9JG0B384gaHrimL8=";
        "x86_64-linux" = "sha256-Oq9xmrdoAJvwQl9WOBkJFhacWHT9JG0B384gaHrimL8=";
      }.${system} or (builtins.throw "Unsupported platform for docker image ${name}, must either be arm64 or amd64 Linux: found ${system}");
      busyboxImage = pkgs.dockerTools.pullImage {
        imageName = name;
        imageDigest = "sha256:50aa4698fa6262977cff89181b2664b99d8a56dbca847bf62f2ef04854597cf8";
        sha256 = platformSha256;
        finalImageTag = tag;
        finalImageName = name;
      };
    in
    pkgs.dockerTools.streamLayeredImage {
      name = name;
      tag = tag;
      fromImage = busyboxImage;
      config.Cmd = [ "busybox" ];
    };

  # Creates the working directory, scripts & initializes the IPFS node.
  # NOTE: This is a global script, which is run by default and is only
  # necessary in scenarios where the systemd service fails.
  setup-working-dir = pkgs.writeShellScriptBin "setup-working-dir.sh" ''
    if [ -e "/app" ]; then
      echo "Working directory already exists."
      exit 0
    fi

    mkdir -p /app/bin
    mkdir -p /app/tmp/kubo

    cd /app
    printf "${procfile.text}" > "${procfile.name}"
    git clone https://github.com/versatus/lasr.git

    cd /app/bin
    printf "${ipfs-config.text}" > "${ipfs-config.name}"
    printf "${start-ipfs.text}" > "${start-ipfs.name}"
    printf "${start-lasr.text}" > "${start-lasr.name}"
    printf "${start-overmind.text}" > "${start-overmind.name}"

    for file in ./*; do
      chmod +x "$file"
    done

    cd /app/tmp/kubo
    export IPFS_PATH=/app/tmp/kubo
    ipfs init

    echo "Done"
    exit 0
  '';
  # Initializes lasr_node environment variables and persists them between system boots.
  # NOTE: This is a global script, which is run by default and is only
  # necessary in scenarios where the systemd service fails.
  init-env = pkgs.writeShellScriptBin "init-env.sh" ''
    if [ -e "\$HOME/.bashrc" ]; then
      echo "Environment already initialized."
      exit 0
    fi

    secret_key=$(lasr_cli wallet new | jq '.secret_key')
    block_path="/app/blocks_processed.dat"
    eth_rpc_url="https://u0anlnjcq5:xPYLI9OMwxRqJZqhfgEiKMeGdpVjGduGKmMCNBsu46Y@u0auvfalma-u0j1mdxq0w-rpc.us0-aws.kaleido.io/" 
    eo_contract=0x563f0efeea703237b32ae7f66123b864f3e46a3c
    compute_rpc_url=ws://localhost:9125 
    storage_rpc_url=ws://localhost:9126
    batch_interval=180
    ipfs_path="/app/tmp/kubo"
    runsc_bin_path="${pkgs.gvisor}/bin/runsc"
    grpcurl_bin_path="${pkgs.grpcurl}/bin/grpcurl"
    echo "set -o noclobber" > ~/.bashrc
    echo "export SECRET_KEY=$secret_key" >> ~/.bashrc
    echo "export BLOCKS_PROCESSED_PATH=$block_path" >> ~/.bashrc
    echo "export ETH_RPC_URL=$eth_rpc_url" >> ~/.bashrc
    echo "export EO_CONTRACT_ADDRESS=$eo_contract" >> ~/.bashrc
    echo "export COMPUTE_RPC_URL=$compute_rpc_url" >> ~/.bashrc
    echo "export STORAGE_RPC_URL=$storage_rpc_url" >> ~/.bashrc
    echo "export BATCH_INTERVAL=$batch_interval" >> ~/.bashrc
    echo "export IPFS_PATH=$ipfs_path" >> ~/.bashrc
    echo "export RUNSC_BIN_PATH=$runsc_bin_path" >> ~/.bashrc
    echo "export GRPCURL_BIN_PATH=$grpcurl_bin_path" >> ~/.bashrc
    echo "[[ \$- == *i* && -f \"\$HOME/.bashrc\" ]] && source \"\$HOME/.bashrc\"" > ~/.bash_profile

    source ~/.bashrc
    echo "Done"
    exit 0
  '';
  # Configure addresses and gateway for IPFS daemon.
  ipfs-config =
    let
      ipfs = "${pkgs.ipfs}/bin/ipfs";
    in
    pkgs.writeTextFile {
      name = "ipfs-config.sh";
      text = ''
        ## The shell in the go-ipfs container is busybox, so a version of ash
        ## Shellcheck might warn on things POSIX sh cant do, but ash can
        ## In Shellcheck, ash is an alias for dash, but busybox ash can do more than dash 
        ## https://github.com/koalaman/shellcheck/blob/master/src/ShellCheck/Data.hs#L134

        ## Uncomment this section to customise the gateway configuration
        # echo "ipfs-config: setting Gateway config"
        # ${ipfs} config --json Gateway '{
        #         "HTTPHeaders": {
        #             "Access-Control-Allow-Origin": [
        #                 "*"
        #             ],
        #         }
        #     }'
        ## Obviously you should use your own domains here, but I thought it instructive to show path and 
        ## subdomain gateways here with the widely known PL domains
        
        ## Disable hole punching
        ${ipfs} config --json Swarm.RelayClient.Enabled true
        ${ipfs} config --json Swarm.EnableHolePunching true

        ## Bind API to all interfaces so that fly proxy for the Kubo API works
        ${ipfs} config Addresses.API --json '["/ip4/0.0.0.0/tcp/5001", "/ip6/::/tcp/5001"]'

        ## Maybe you need to listen on IPv6 too? Some clouds use it for internal networking
        ${ipfs} config --json Addresses.Gateway '["/ip4/0.0.0.0/tcp/8080", "/ip6/::/tcp/8080"]'

        ## In fly.io there's no way to know the public IPv4 so it has to be manually configured to be announced
        ## Note that it must be 1-1, you can't point at multiple go-ipfs nodes and expect it to work
        ${ipfs} config --json Addresses.AppendAnnounce '["/ip4/167.99.20.121/tcp/4001", "/ip4/167.99.20.121/tcp/4002/ws", "/ip6/2a09:8280:1::1c:fb3f/tcp/4001", "/ip6/2a09:8280:1::1c:fb3f/tcp/4002/ws", "/dns4/versatus-lasr-ipfs.fly.dev/tcp/443/wss"]'
        ${ipfs} config Swarm.Transports.Network.Websocket --json true
        ${ipfs} config Swarm.Transports.Network.WebTransport --json true
        ${ipfs} config --json Addresses.Swarm '["/ip4/0.0.0.0/tcp/4001", "/ip4/0.0.0.0/tcp/4002/ws", "/ip4/0.0.0.0/udp/4003/quic/webtransport", "/ip6/::/tcp/4001", "/ip6/::/tcp/4002/ws", "/ip6/::/udp/4003/quic/webtransport", "/ip4/0.0.0.0/udp/4001/quic", "/ip6/::/udp/4001/quic"]'
        ${ipfs} config Swarm.ResourceMgr.Enabled --json true
        ${ipfs} config --json API.HTTPHeaders.Access-Control-Allow-Origin '["http://167.99.20.121:5001", "http://127.0.0.1:5001", "https://webui.ipfs.io"]'
        ${ipfs} config --json API.HTTPHeaders.Access-Control-Allow-Methods '["PUT", "POST"]'
        echo "Finished configuring IPFS."
      '';
      executable = true;
      destination = "/app/bin/ipfs-config.sh";
    };
  # Starts kubo IPFS daemon.
  start-ipfs = pkgs.writeTextFile {
    name = "start-ipfs.sh";
    text = ''
      IPFS_PATH=/app/tmp/kubo ipfs daemon
    '';
    executable = true;
    destination = "/app/bin/start-ipfs.sh";
  };
  # Starts the lasr_node from the release build specified by the nix flake.
  start-lasr = pkgs.writeTextFile {
    name = "start-lasr.sh";
    text = ''
      PREFIX=/app/lasr
      cd $PREFIX

      # For local development:
      # The default behaviour will use the binary specified by the nix flake.
      # This is important because it retains the flake.lock versioning.
      # If this is not important to you, exchange the call to lasr_node
      # with the following:
      # ./target/release/lasr_node
      lasr_node
    '';
    executable = true;
    destination = "/app/bin/start-lasr.sh";
  };
  # Main process script. Re/starts the node and dependencies.
  start-overmind = pkgs.writeTextFile {
    name = "start-overmind.sh";
    text = ''
      OVERMIND_CAN_DIE=reset overmind start -D -N /app/Procfile
    '';
    executable = true;
    destination = "/app/bin/start-overmind.sh";
  };
  # Overmind's configuration file.
  # The destination path is automatically created in the nix-store.
  procfile = pkgs.writeTextFile {
    name = "Procfile";
    text = ''
      ipfs: /app/bin/start-ipfs.sh
      lasr: sleep 5 && /app/bin/start-lasr.sh
    '';
    destination = "/app/Procfile";
  };
in
{
  # Packages that will be available on the resulting NixOS system.
  # Please keep these in alphabetical order so packages are easy to find.
  environment.systemPackages = with pkgs; [
    curl
    docker
    git
    grpcurl
    gvisor
    jq
    kubo
    overmind
    tmux
    lasr_node
    lasr_cli
  ] ++ [
    init-env
    ipfs-config
    procfile
    setup-working-dir
    start-ipfs
    start-lasr
    start-overmind
    start-pd-server
    start-tikv-server
  ];

  # Enable docker socket
  virtualisation.docker.enable = true;
  users.users.root.extraGroups = [ "docker" ];
  # Automatically start the pd-server & tikv-server on server start
  virtualisation.oci-containers = {
    backend = "docker";
    containers = {
      pd-server = {
        image = "pingcap/pd:latest";
        imageFile = pd-image;
        extraOptions = [
          "--network=host"
        ];
        cmd = [
          "--data-dir=/pd1"
          "--client-urls=http://0.0.0.0:2379"
          "--peer-urls=http://0.0.0.0:2380"
          "--advertise-client-urls=http://0.0.0.0:2379"
          "--advertise-peer-urls=http://0.0.0.0:2380"
        ];
      };
      tikv-server = {
        dependsOn = [ "pd-server" ];
        image = "pingcap/tikv:latest";
        imageFile = tikv-image;
        extraOptions = [
          "--network=host"
        ];
        cmd = [
          "--addr=127.0.0.1:20160"
          "--advertise-addr=127.0.0.1:20160"
          "--data-dir=/tikv"
          "--pd=http://127.0.0.1:2379"
        ];
      };
    };
  };

  # Node services that will run on server start
  systemd.user.services = {
    init-env = {
      description = "Initializes lasr_node environment variables and persists them between system boots.";
      script = ''
        if [ -f "\$HOME/.bashrc" ]; then
          echo "Environment already initialized."
          exit 0
        fi

        secret_key=$(${pkgs.lasr_cli}/bin/lasr_cli wallet new | ${pkgs.jq}/bin/jq '.secret_key')
        block_path="/app/blocks_processed.dat"
        eth_rpc_url="https://u0anlnjcq5:xPYLI9OMwxRqJZqhfgEiKMeGdpVjGduGKmMCNBsu46Y@u0auvfalma-u0j1mdxq0w-rpc.us0-aws.kaleido.io/" 
        eo_contract=0x563f0efeea703237b32ae7f66123b864f3e46a3c
        compute_rpc_url=ws://localhost:9125
        storage_rpc_url=ws://localhost:9126
        batch_interval=180
        ipfs_path="/app/tmp/kubo"
        runsc_bin_path="${pkgs.gvisor}/bin/runsc"
        grpcurl_bin_path="${pkgs.grpcurl}/bin/grpcurl"
        echo "set -o noclobber" > ~/.bashrc
        echo "export SECRET_KEY=$secret_key" >> ~/.bashrc
        echo "export BLOCKS_PROCESSED_PATH=$block_path" >> ~/.bashrc
        echo "export ETH_RPC_URL=$eth_rpc_url" >> ~/.bashrc
        echo "export EO_CONTRACT_ADDRESS=$eo_contract" >> ~/.bashrc
        echo "export COMPUTE_RPC_URL=$compute_rpc_url" >> ~/.bashrc
        echo "export STORAGE_RPC_URL=$storage_rpc_url" >> ~/.bashrc
        echo "export BATCH_INTERVAL=$batch_interval" >> ~/.bashrc
        echo "export IPFS_PATH=$ipfs_path" >> ~/.bashrc
        echo "export RUNSC_BIN_PATH=$runsc_bin_path" >> ~/.bashrc
        echo "export GRPCURL_BIN_PATH=$grpcurl_bin_path" >> ~/.bashrc
        echo "[[ \$- == *i* && -f \"\$HOME/.bashrc\" ]] && source \"\$HOME/.bashrc\"" > ~/.bash_profile
        echo "Successfully initialized lasr_node environment."
      '';
      wantedBy = [ "default.target" ];
    };
    ipfs-start = {
      description = "Setup and start IPFS daemon.";
      preStart = ''
        if [ ! -e "/app/tmp/kubo" ]; then
          mkdir -p /app/tmp/kubo
          cd /app/tmp/kubo
          export IPFS_PATH=/app/tmp/kubo
          "${pkgs.kubo}/bin/ipfs" init
          echo "Initialized IPFS."
          IPFS_CONFIG="${ipfs-config}/app/bin/ipfs-config.sh"
          echo "Configuring IPFS from ipfs-config path: $IPFS_CONFIG"
          "$IPFS_CONFIG"
          echo "IPFS ready."
        else
          echo "IPFS already initialized and configured. IPFS ready."
        fi
      '';
      script = ''
        sleep 2
        IPFS_PATH=/app/tmp/kubo "${pkgs.kubo}/bin/ipfs" daemon
      '';
      wantedBy = [ "node-start.service" ];
    };
    node-start = {
      description = "Start the lasr_node process.";
      after = [ "init-env.service" "ipfs-start.service" ];
      preStart =
        let
          # nix-store paths are resolved on user login, but here we have to give the absolute paths.
          git = "${pkgs.git}/bin/git";
          docker = "${pkgs.docker}/bin/docker";
          sudo = "/run/wrappers/bin/sudo";
          tar = "/run/current-system/sw/bin/tar";
        in
        ''
          if [ ! -e "/app/bin" ]; then
            echo "Setting up working directory.."
            mkdir -p /app/bin
            mkdir -p /app/payload
            mkdir -p /app/base_image/busybox

            cd /app
            printf "${procfile.text}" > "${procfile.name}"
            ${git} clone https://github.com/versatus/lasr.git

            cd /app/base_image/busybox
            mkdir --mode=0755 rootfs
            ${busybox-stream} | ${docker} image load
            ${docker} export $(${docker} create busybox) | ${sudo} ${tar} -xf - -C rootfs --same-owner --same-permissions

            cd /app/bin
            printf "${start-ipfs.text}" > "${start-ipfs.name}"
            printf "${start-lasr.text}" > "${start-lasr.name}"
            printf "${start-overmind.text}" > "${start-overmind.name}"

            for file in ./*; do
              chmod +x "$file"
            done

            # wait for busybox to set up the root filesystem for the program env
            while [ ! -e "/app/base_image/busybox/rootfs/bin" ]; do
              echo "Waiting for busybox filesystem to be ready, sleeping for 2s..."
              sleep 2
            done

            echo "Working directory '/app' is ready."
          else
            echo "Working directory already exists."
          fi
        '';
      script = ''
        # grace period for all services to finish setup
        sleep 5

        source "$HOME/.bashrc"
        cd /app
        "${pkgs.lasr_node}/bin/lasr_node"
      '';
      wantedBy = [ "default.target" ];
      serviceConfig = {
        Restart = "always";
        RestartSec = 5;
      };
    };
  };

  # Before changing, read this first:
  # https://nixos.org/manual/nixos/stable/options.html#opt-system.stateVersion
  system.stateVersion = "24.04";
}
