#!/bin/bash

REQUEST_ID=$1

STATUS=$(./poll-blob-status.sh $REQUEST_ID)

BATCH_HEADER_HASH=$(echo $STATUS | jq -r .info.blobVerificationProof.batchMetadata.batchHeaderHash)
BLOB_INDEX=$(echo $STATUS | jq -r .info.blobVerificationProof.blobIndex)

cd ~/Projects/verse/protocol/lasr/eigenda

echo $BATCH_HEADER_HASH
echo $BLOB_INDEX

grpcurl \
  -import-path ./api/proto \
  -proto ./api/proto/disperser/disperser.proto \
  -d '{"batch_header_hash": "'"$BATCH_HEADER_HASH"'", "blob_index":'"$BLOB_INDEX"'}' \
  disperser-holesky.eigenda.xyz:443 disperser.Disperser/RetrieveBlob | \
  jq -r .data | \
  kzgpad -d -
