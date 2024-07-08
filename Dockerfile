# syntax=docker/dockerfile:1

ARG PLATFORM=linux/amd64
ARG BASE_IMAGE=busybox
ARG RUST_VERSION=1.75.0
ARG UBUNTU_VERSION=23.10
ARG GRPCURL_VERSION=v1.8.9
ARG GRPCURL=grpcurl_1.8.9_linux_x86_64.tar.gz

###############################
FROM --platform=${PLATFORM} ubuntu:${UBUNTU_VERSION} as OS_BUILDER
RUN apt-get update && apt-get update && apt-get install -y wget
WORKDIR /tmp
RUN wget https://go.dev/dl/go1.21.1.linux-amd64.tar.gz
RUN tar -xvf go1.21.1.linux-amd64.tar.gz
RUN mv go /usr/local
RUN GOBIN=/usr/local/bin/ /usr/local/go/bin/go install github.com/canonical/chisel/cmd/chisel@latest
WORKDIR /rootfs
RUN chisel cut --release ubuntu-22.04 --root /rootfs \
    base-files_base \
    base-files_release-info \
    ca-certificates_data \
    libgcc-s1_libs \
    libc6_libs

#
# Base Image
#
# FROM ubuntu:${UBUNTU_VERSION} as base_image
# USER root
# ARG BASE_IMAGE

# RUN apt-get update && \
#     apt-get install -y ca-certificates curl && \
#     install -m 0755 -d /etc/apt/keyrings && \
#     curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc && \
#     chmod a+r /etc/apt/keyrings/docker.asc && \
#     echo \
#         "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu \
#         $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | \
#         tee /etc/apt/sources.list.d/docker.list > /dev/null && \
#     apt-get update && \
#     apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin tree

# RUN mkdir /app
# RUN cd /app \
#     && mkdir --mode=0755 rootfs
# # RUN systemctl start docker && sleep 5
# # RUN nohup dockerd >/dev/null 2>&1 && \ 
# RUN docker export $(docker create ${BASE_IMAGE}) | tar -xf - -C rootfs --same-owner --same-permissions

#
# GVisor
#
FROM --platform=${PLATFORM} ubuntu:${UBUNTU_VERSION} as gvisor

WORKDIR /app

RUN apt-get update && apt-get install -y wget

ENV GVISOR_ARCH=x86_64
ENV GVISOR_URL=https://storage.googleapis.com/gvisor/releases/release/latest/${GVISOR_ARCH}

RUN wget ${GVISOR_URL}/runsc ${GVISOR_URL}/runsc.sha512 ${GVISOR_URL}/containerd-shim-runsc-v1 ${GVISOR_URL}/containerd-shim-runsc-v1.sha512 && \
    sha512sum -c runsc.sha512 -c containerd-shim-runsc-v1.sha512 && \
    rm -f *.sha512 && \
    chmod a+rx runsc containerd-shim-runsc-v1

#
# EIGENDA
#
FROM --platform=${PLATFORM} ubuntu:${UBUNTU_VERSION} as eigenda

RUN apt-get update && apt-get install -y git \
    && mkdir -p /app \
    && git clone https://github.com/Layr-Labs/eigenda.git /app/eigenda

#
# GRPCURL
#
FROM --platform=${PLATFORM} ubuntu:${UBUNTU_VERSION} as grpcurl

RUN apt-get update && apt-get install -y wget

ARG GRPCURL_VERSION
ARG GRPCURL

ENV GRPCURL_URL=https://github.com/fullstorydev/grpcurl/releases/download/${GRPCURL_VERSION}/${GRPCURL}

WORKDIR /

RUN wget "$GRPCURL_URL" && tar -xvf "$GRPCURL" && mv grpcurl /usr/local/bin/grpcurl

###############################
FROM --platform=${PLATFORM} rust:${RUST_VERSION} as APP_PLANNER
WORKDIR /usr/local/src
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

###############################
FROM --platform=${PLATFORM} rust:${RUST_VERSION} as APP_CACHER
WORKDIR /usr/local/src
RUN cargo install cargo-chef
COPY --from=APP_PLANNER /usr/local/src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

###############################
FROM --platform=${PLATFORM} rust:${RUST_VERSION} as APP_BUILDER
COPY . /usr/local/src
WORKDIR /usr/local/src
COPY --from=APP_CACHER /usr/local/src/target target
COPY --from=APP_CACHER $CARGO_HOME $CARGO_HOME
RUN cargo build --release
RUN chmod 755 /usr/local/src/target/release/main

# Final Image
FROM --platform=${PLATFORM} ubuntu:${UBUNTU_VERSION} as final

RUN apt-get update && \
    apt-get install -y apt-transport-https ca-certificates curl gnupg tree sudo 

RUN echo "ubuntu ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers


# Install Docker
# RUN install -m 0755 -d /etc/apt/keyrings && \
#     curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc && \
#     chmod a+r /etc/apt/keyrings/docker.asc

# RUN echo \
#   "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu \
#   $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | \
#   tee /etc/apt/sources.list.d/docker.list > /dev/null

# RUN apt-get update && \ 
#     apt-get install -y docker-ce docker-ce-cli containerd.io

# Install gvisor
# RUN curl -fsSL https://gvisor.dev/archive.key | gpg --dearmor -o /usr/share/keyrings/gvisor-archive-keyring.gpg && \
#     echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/gvisor-archive-keyring.gpg] https://storage.googleapis.com/gvisor/releases release main" | tee /etc/apt/sources.list.d/gvisor.list > /dev/null

# RUN sudo apt-get update && apt-get install -y runsc && \
#     sudo ln -s /usr/bin/runsc /usr/local/bin/runsc


USER ubuntu
RUN sudo mkdir /app
WORKDIR /app
COPY --from=APP_BUILDER /usr/local/src/target/release /app
COPY --from=grpcurl /usr/local/bin/grpcurl /usr/local/bin/grpcurl
COPY --from=eigenda /app/eigenda /app/eigenda
COPY --from=gvisor /app/containerd-shim-runsc-v1 /usr/local/bin/
COPY --from=gvisor /app/runsc /usr/local/bin/

COPY base_image.tar.gz /app/base_image.tar.gz
COPY payload /app/payload

RUN sudo tar -xzf /app/base_image.tar.gz && \
    sudo chown -R root:root /app/base_image && \
    sudo chmod -R 777 /app/base_image && \
    sudo mkdir /app/containers && \
    sudo mv /app/main /usr/local/bin/main && \
    sudo mv /app/cli /usr/local/bin/cli

RUN sudo cli wallet new --save

ENTRYPOINT [ "/usr/local/bin/main" ]
