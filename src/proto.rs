// proto.rs - Consolidated Protobuf Definitions for VORTEX
// This module centralizes the `tonic::include_proto!` macro to prevent 
// duplicate compilations of the protobuf types across different modules (e.g. swarm.rs, worker.rs)

pub mod vortex_swarm {
    tonic::include_proto!("vortex.swarm");
}

pub use vortex_swarm::*;
