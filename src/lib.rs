//! Disclosed-baseline benchmark harness for stock rust-libp2p gossipsub
//! at large (4-10 MB) message sizes.
//!
//! Every gossipsub knob is set explicitly and emitted alongside the
//! results, so any measured number is attributable to a disclosed
//! configuration. Links are emulated in-process: a fluid-model shaper
//! applies serialization delay (bandwidth) plus one-way propagation
//! delay on the receive path of every connection.

pub mod config;
pub mod error;
pub mod report;
pub mod runner;
pub mod shaping;
pub mod topology;
