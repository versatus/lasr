# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.75.0
ARG UBUNTU_VERSION=22.04
ARG GRPCURL_VERSION=v1.8.9
ARG GRPCURL=grpcurl_1.8.9_linux_x86_64.tar.gz

###############################
FROM ubuntu:${UBUNTU_VERSION} as OS_BUILDER
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
# EIGENDA
#
FROM ubuntu:${UBUNTU_VERSION} as eigenda

USER root

RUN apt-get update && apt-get install -y git \
    && mkdir -p /app \
    && git clone https://github.com/Layr-Labs/eigenda.git /app/eigenda

#
# GRPCURL
#
FROM ubuntu:${UBUNTU_VERSION} as grpcurl

RUN apt-get update && apt-get install -y wget

ARG GRPCURL_VERSION
ARG GRPCURL

ENV GRPCURL_URL=https://github.com/fullstorydev/grpcurl/releases/download/${GRPCURL_VERSION}/${GRPCURL}

WORKDIR /

RUN wget "$GRPCURL_URL" && tar -xvf "$GRPCURL" && mv grpcurl /usr/local/bin/grpcurl

###############################
FROM rust:${RUST_VERSION} as APP_PLANNER
WORKDIR /usr/local/src
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust:1.75.0 as APP_CACHER
WORKDIR /usr/local/src
RUN cargo install cargo-chef
COPY --from=APP_PLANNER /usr/local/src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM rust:${RUST_VERSION} as APP_BUILDER
COPY . /usr/local/src
WORKDIR /usr/local/src
COPY --from=APP_CACHER /usr/local/src/target target
COPY --from=APP_CACHER $CARGO_HOME $CARGO_HOME
RUN cargo build --release
RUN chmod 755 /usr/local/src/target/release/main

# FROM scratch
FROM ubuntu:${UBUNTU_VERSION} as final

COPY --from=grpcurl /usr/local/bin/grpcurl /usr/local/bin/grpcurl

RUN mkdir /app

WORKDIR /app
COPY --from=APP_BUILDER /usr/local/src/target/release/main /app/main
COPY --from=eigenda /app/eigenda /app/eigenda


ENTRYPOINT [ "/app/main" ]
