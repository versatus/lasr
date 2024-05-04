#!/bin/bash

# Environment variables
export SECRET_KEY="954d7e1053dffa010466cc962d45cd86eee3630d8c82ef67de1e7439ac157e92"
export BLOCKS_PROCESSED_PATH="/opt/versatus/home/versatus-lasr/blocks_processed.dat"
export ETH_RPC_URL="https://u0q9d8a4y9:1xFxpYz3hFXUjzjO7QVKOe6wjDkyXsK3s9SbH8acR5Y@u0v4deab9j-u0ghk9j0sc-rpc.us0-aws.kaleido.io/"
export EO_CONTRACT_ADDRESS="0xca3ed4ab07ef6b98d797a35a5aef301ec24a829f"
export COMPUTE_RPC_URL="ws://localhost:9125"
export STORAGE_RPC_URL="ws://localhost:9126"
export BATCH_INTERVAL="180"

# Run the binary
target/debug/lasr_node