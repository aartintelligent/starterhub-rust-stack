//! Business handlers, one module per resource.
//!
//! Handlers stay thin: extract the input through [`crate::extract`], call
//! a service, map the result. Business logic belongs to
//! [`crate::service`]. Technical endpoints (health probes, fallback) are
//! properties of the routing table and live in [`crate::router`], not
//! here.
//!
//! Add one module per resource and re-export it here:
//!
//! ```text
//! mod subnet;
//! pub use subnet::*;
//! ```
