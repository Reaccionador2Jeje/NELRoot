//! Public NELRoot core library API.
//!
//! The CLI is one consumer of these components; exposing the core as a library
//! keeps loader, stack, memory and address-space APIs available to other tools
//! without pretending that guest instruction execution is implemented.

pub mod core;
