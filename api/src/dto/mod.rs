//! Data transfer objects: the wire format of the API.
//!
//! Request payloads and response bodies live here, decoupled from the
//! database entities (the `entity` crate), so the public contract can
//! evolve independently of the internals.
//!
//! Add one module per resource and re-export it here:
//!
//! ```text
//! mod subnet;
//! pub use subnet::*;
//! ```
