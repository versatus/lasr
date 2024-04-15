#!/bin/bash

DATA=$1

ENCODED_DATA=$(./encode-blob.sh "$DATA")

cd ~/Projects/verse/protocol/lasr/eigenda

grpcurl \
  -import-path ./api/proto \
  -proto ./api/proto/disperser/disperser.proto \
  -d '{"data": "'"$ENCODED_DATA"'"}' \
  disperser-holesky.eigenda.xyz:443 disperser.Disperser/DisperseBlob
