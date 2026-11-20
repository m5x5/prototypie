//! Run the API standalone on 127.0.0.1:3126 (PROTOTYPIE_ADDR overrides; never :3124, the app's port): `PROTOTYPIE_DATA=/tmp/x cargo run -p proto-server --example serve [-- --import <state.json>]`.
//! With --import, a copy of that state.json is imported once into the data dir (never written back).

use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let data = PathBuf::from(std::env::var("PROTOTYPIE_DATA").map_err(|_| anyhow::anyhow!("set PROTOTYPIE_DATA"))?);
    std::fs::create_dir_all(&data)?;
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--import") {
        let src = PathBuf::from(args.get(i + 1).ok_or_else(|| anyhow::anyhow!("--import <state.json>"))?);
        let outcome = proto_core::import_reference(&src, &data)?;
        eprintln!("import: {outcome:?}");
    }
    let store = tokio::task::spawn_blocking(move || proto_core::Store::open(&data)).await??;
    let dir = store.data_dir().expect("on disk").to_path_buf();
    let mut cfg = proto_server::ServerConfig::new(dir);
    // Never :3124: that port belongs to the app's embedded server.
    cfg.addr = "127.0.0.1:3126".parse()?;
    if let Ok(a) = std::env::var("PROTOTYPIE_ADDR") {
        cfg.addr = a.parse()?;
    }
    // The notes service is this server's own unless `PROTOTYPIE_NOTES_URL` names another one.
    cfg.notes_url = std::env::var("PROTOTYPIE_NOTES_URL").unwrap_or_else(|_| format!("http://{}", cfg.addr));
    let handle = proto_server::serve(cfg, store.clone(), None).await?;
    eprintln!("proto-server on http://{}", handle.addr);
    tokio::signal::ctrl_c().await?;
    drop(handle);
    tokio::task::spawn_blocking(move || store.flush()).await??;
    Ok(())
}
