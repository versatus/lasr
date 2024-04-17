#!/bin/bash

REQUEST=$1
REQUEST_ID=$(jq $REQUEST -r .requestId)

cd ~/Projects/verse/protocol/lasr/eigenda

grpcurl \
  -import-path ./api/proto \
  -proto ./api/proto/disperser/disperser.proto \
  -d '{"request_id": "'"$REQUEST_ID"'"}' \
  disperser-holesky.eigenda.xyz:443 disperser.Disperser/GetBlobStatus
