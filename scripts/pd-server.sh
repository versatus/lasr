#!/bin/bash

docker run -d --name pd-server --network host pingcap/pd:latest \
        --name="pd1" \
        --data-dir="/pd1" \
        --client-urls="http://0.0.0.0:2379" \
        --peer-urls="http://0.0.0.0:2380" \
        --advertise-client-urls="http://0.0.0.0:2379" \
        --advertise-peer-urls="http://0.0.0.0:2380"