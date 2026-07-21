//! Integration test of the PostgreSQL pool bootstrap.
//!
//! The happy path needs a live server and belongs to the running
//! application; what must be guaranteed here is the boot-time contract:
//! an unreachable database surfaces as an error, fast.

use common::config::{Postgresql, PostgresqlPool};
use common::infrastructure::postgresql;

/// `connect` applies the configured timeouts and surfaces an unreachable
/// server as an error instead of hanging the boot.
#[tokio::test]
async fn unreachable_server_fails_fast() {
    // Port 1 is never a PostgreSQL server: the connection is refused
    // immediately, and the 1-second timeouts bound the worst case.
    let config = Postgresql {
        host: "127.0.0.1".into(),
        port: 1,
        username: "user".into(),
        password: "secret".into(),
        database: "app".into(),
        pool: PostgresqlPool {
            max_connections: 1,
            min_connections: 0,
            connect_timeout: 1,
            acquire_timeout: 1,
            idle_timeout: 60,
            max_lifetime: 60,
        },
    };

    assert!(postgresql::connect(&config).await.is_err());
}
