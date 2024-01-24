# From https://github.com/ipfs-shipyard/go-ipfs-docker-examples/
FROM ipfs/kubo:v0.26.0

ADD ipfs-config.sh /container-init.d/ipfs-config.sh
RUN chmod a+x /container-init.d/ipfs-config.sh

EXPOSE 4001/udp
EXPOSE 4001/tcp
EXPOSE 4002/tcp
# 👇 WebTransport
EXPOSE 4003/udp
EXPOSE 8080
