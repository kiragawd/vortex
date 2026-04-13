// proto.rs - Consolidated Protobuf Definitions for RYUO
// This module centralizes the `tonic::include_proto!` macro to prevent 
// duplicate compilations of the protobuf types across different modules (e.g. swarm.rs, worker.rs)

pub mod ryuo_swarm {
    tonic::include_proto!("ryuo.swarm");
}

pub use ryuo_swarm::*;
