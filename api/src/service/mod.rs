//! Business logic of the API.
//!
//! Follows the CQS split promoted by the sea-orm examples: read operations
//! live on [`Query`], write operations on [`Mutation`]. Handlers never touch
//! the database directly, they always go through one of the two.

mod mutation;
mod query;

pub use mutation::*;
pub use query::*;
