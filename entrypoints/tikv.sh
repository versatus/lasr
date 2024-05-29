#!/bin/sh
set -e

exec ./tikv-server \
    --addr 0.0.0.0:20160 \
    --status-addr 0.0.0.0:20180 \
    --advertise-addr `cat /etc/hostname`:20160 \
    --pd-endpoints pd.tikv:2379
