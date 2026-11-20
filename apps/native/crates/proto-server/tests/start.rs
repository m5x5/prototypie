//! The embed API: bind, answer, shut down; a second start on the same port fails at once.

use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn start_serves_and_shuts_down() {
    let dir = tempfile::tempdir().unwrap();
    let store = proto_core::Store::open(dir.path()).unwrap();
    let ai = proto_ai::Ai::new(dir.path().to_path_buf(), None, Arc::new(proto_ai::NoNotifier));
    let opts = proto_server::ServeOpts { addr: "127.0.0.1:0".parse().unwrap(), ..Default::default() };
    let handle = proto_server::start(store.clone(), ai.clone(), opts).unwrap();
    let addr = handle.addr;
    let body = reqwest::get(format!("http://{addr}/api/state")).await.unwrap().text().await.unwrap();
    assert!(body.contains("\"records\""));
    let again = proto_server::start(store, ai, proto_server::ServeOpts { addr, ..Default::default() });
    assert!(again.is_err());
    assert!(!handle.sharing().borrow().sharing);
    handle.shutdown().await;
    assert!(reqwest::get(format!("http://{addr}/api/state")).await.is_err());
}
