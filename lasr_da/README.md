This is a Work In Progress Rust implementation of a quick and dirty 
RPC client for connecting to, writing to, retrieving from EigenDA, as well as 
checking status of writes
<break>
[EigenDA Github](https://github.com/Layr-Labs/eigenda/tree/master) 
<break>
[EigenDA Docs](https://docs.eigenlayer.xyz/eigenda-guides/eigenda-rollup-user-guides)

This library is dependent on:

1. grpcurl. To install grpcurl follow these [instructions](https://github.com/fullstorydev/grpcurl#installation)
grpcurl is a command line tool for interacting with gRPC servers. grpcurl is 
dependent on the Go SDK. Preferably version 1.13 or newer of 
Go SDK as it will give you less problems. 
2. EigenDA proto-buffers.

After installing grpcurl and ensuring that it is in your `PATH` by calling 
`grpc --help` you will want to follow these steps to use this library:


*Unfortunately EigenDA gRPC server(s) are not currently working with 
[tonic](https://crates.io/crates/tonic), the Rust native gRPC implementation
though we do expect that to get resolved in the near future, and will 
update this project to use native Rust gRPC rather than forking a command 
to the command line to send the gRPC requests*

### Clone EigenDA Repository into the root directory of this project
```bash
git clone https://github.com/Layr-Labs/eigenda.git /path/to/project/root/directory
```

### Add this crate to your Cargo.toml
```toml
[dependencies]
//...Other dependencies
eigenda_client = "0.1.0"
```

### Using the client
```rust

fn main() -> Result<(), Error> {

    let mut client = EigenDaGrpcClientBuilder::new()
        .proto_path("./eigenda/api/proto/disperser/disperser.proto")
        .server_address("disperser-goerli.eigenda.xyz:443")
        .adversary_threshold(adversary_threshold)
        .quorum_threshold(quorum_threshold)
        .blob_cache(LinkedHashSet::new())
        .build()?;


    let arbitrary_data = "ArbitraryData";
    let blob_response = client.disperse_blob(arbitrary_data, 0).unwrap();
    let mut blob_status = client.get_blob_status(&blob_response.request_id).unwrap(); 

    // You will likely want to actually poll in a separate thread as this blocks
    while blob_status.status() != &BlobResult::Confirmed {
        thread::sleep(Duration::from_secs(30));
        blob_status = client.get_blob_status(&blob_response.request_id).unwrap();
    }
    
    // When the status returns you will likely want to store the status 
    // somewhere and poll for the actual blob in a separate thread as this
    // blocks
    let batch_header_hash = blob_status.batch_header_hash();
        let blob = client.retrieve_blob(batch_header_hash, blob_index);
    }
}
```

### Features

| Feature | Status |
|---------|--------|
| Disperse Blobs | :white_check_mark:|
| Retrieve Blob Status | :white_check_mark: |
| Cache Blob Status When Confirmed | :white_check_mark: |
| Retrieve Blobs Once Confirmed | :white_check_mark: |
| Async Blob Dispersal | :x: |
| Async Blob Retrieval | :x: |
| Non-Blocking Polling for Blob Status | :x: |
| Concurrent Blob Dispersal | :x: |
| Concurrent Blob Status Checking | :x: |
| Concurrent Blob Retrieval | :x: |
| Native Rust gRPC Requests with Tonic | :x: |

### Status

While this project is a WIP, it does work for the absolute most basic 
use cases, however, we would caution you against using this in production 
as of right now. Anticipation is that this will be ready to move to 
production by end of 2023.
