use anyhow::Result;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let mut manifests_dir =
        std::env::var("OWASIDEA_MANIFESTS").unwrap_or_else(|_| "../manifests".to_string());
    let mut addr = std::env::var("HOST_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_string());

    // Minimal arg parsing: first positional = manifests dir
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--manifests" | "--manifest-dir" => {
                if let Some(v) = args.next() {
                    manifests_dir = v;
                }
            }
            "--addr" | "--http-addr" => {
                if let Some(v) = args.next() {
                    addr = v;
                }
            }
            other => {
                manifests_dir = other.to_string();
            }
        }
    }

    owasidea_host::run(PathBuf::from(manifests_dir), addr).await
}
