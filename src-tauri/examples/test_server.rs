//! Local download server for trying the app by hand.
//!
//! Run from `src-tauri`:  cargo run --example test_server
//! Then add e.g. http://127.0.0.1:8787/files/big.bin in BDM.

#[path = "../testsupport/server.rs"]
mod server;

use std::time::Duration;

use server::{FileSpec, TestServer, test_data};

#[tokio::main]
async fn main() {
    let srv = TestServer::start_on("127.0.0.1:8787").await;
    let s = &srv.state;
    // ~20 MB/s per connection: slow enough to watch, pause and resume.
    s.add("big.bin", FileSpec::new(test_data(100 * 1024 * 1024, 1)).etag("big-v1").delay(Duration::from_millis(1)));
    s.add("slow.bin", FileSpec::new(test_data(20 * 1024 * 1024, 2)).etag("slow-v1").delay(Duration::from_millis(40)));
    s.add("noranges.bin", FileSpec::new(test_data(15 * 1024 * 1024, 3)).no_ranges().delay(Duration::from_millis(10)));
    s.add("flaky.bin", FileSpec::new(test_data(10 * 1024 * 1024, 4)).etag("flaky").drop_after(300_000, 20).delay(Duration::from_millis(5)));
    s.add("small.txt", FileSpec::new(b"Hello from the BDM test server.\n".to_vec()));
    s.add("archive.zip", FileSpec::new(test_data(8 * 1024 * 1024, 5)).etag("zip-v1"));

    println!("BDM test server listening on http://{}", srv.addr);
    for name in ["big.bin", "slow.bin", "noranges.bin", "flaky.bin", "small.txt"] {
        println!("  {}", srv.url(&format!("/files/{name}")));
    }
    println!("  {}  (expired link)", srv.url("/status/410"));
    println!("  {}  (redirects to small.txt)", srv.url("/redirect/small.txt"));
    // Runs until the process is stopped.
    std::future::pending::<()>().await;
}
