//! 在 `web/dist` 尚未构建时生成占位页，保证 `cargo check/test` 不被阻塞。

use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../../web/dist");
    let dist = Path::new("../../web/dist");
    if !dist.join("index.html").exists() {
        let _ = fs::create_dir_all(dist);
        let _ = fs::write(
            dist.join("index.html"),
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentBeacon</title></head>\
             <body><p>前端资源尚未构建，请运行 pnpm build:web。</p></body></html>",
        );
    }
}
