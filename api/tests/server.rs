//! Integration tests of the HTTP server bootstrap.
//!
//! Unlike the router tests, these exercise the real network path: the
//! server binds an OS-assigned port, answers a raw TCP client, and stops
//! gracefully when its shutdown future resolves.

use std::time::Duration;

use api::server::Server;
use sea_orm::{DatabaseBackend, MockDatabase};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;

/// Builds a server on `addr`, backed by a mock database.
fn server(addr: &str) -> Server {
    // Global TRACE-level test subscriber (first caller wins), so the log
    // statements of the exercised paths are fully evaluated.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let conn = MockDatabase::new(DatabaseBackend::Postgres).into_connection();

    Server::new(addr, conn, "under-test", false, Duration::from_secs(5))
}

/// The server binds port 0, answers a real request on `/livez` through
/// the full middleware stack, then stops cleanly on shutdown.
#[tokio::test]
async fn serves_and_stops_gracefully() {
    let bound = server("127.0.0.1:0")
        .bind()
        .await
        .expect("port 0 must bind");
    let addr = bound.local_addr();

    // Drive the accept loop from a task, keeping the shutdown trigger here.
    let (shutdown, on_shutdown) = oneshot::channel::<()>();
    let handle = tokio::spawn(bound.serve(async move {
        let _ = on_shutdown.await;
    }));

    // Raw TCP client: no HTTP dependency needed for one request, and it
    // proves the wire format end to end (status line included).
    let mut stream = TcpStream::connect(addr).await.expect("server must accept");
    stream
        .write_all(b"GET /livez HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\n\r\n")
        .await
        .expect("request must be writable");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("response must be readable");

    assert!(response.starts_with("HTTP/1.1 200"), "got: {response}");
    assert!(response.contains(r#"{"status":"ok"}"#), "got: {response}");

    // The serve future must resolve cleanly once shutdown fires.
    shutdown.send(()).expect("server must still be running");
    handle
        .await
        .expect("serve must not panic")
        .expect("serve must stop cleanly");
}

/// An unbindable address fails fast with an error naming the culprit.
#[tokio::test]
async fn bind_failure_names_the_address() {
    let Err(error) = server("999.999.999.999:0").bind().await else {
        panic!("nonsense address must not bind");
    };

    assert!(
        format!("{error:#}").contains("failed to bind 999.999.999.999:0"),
        "got: {error:#}"
    );
}

/// The `run` convenience surfaces bind failures exactly like `bind`.
#[tokio::test]
async fn run_surfaces_bind_failures() {
    let error = server("999.999.999.999:0")
        .run(std::future::ready(()))
        .await
        .expect_err("nonsense address must not bind");

    assert!(
        format!("{error:#}").contains("failed to bind 999.999.999.999:0"),
        "got: {error:#}"
    );
}
