//! The page comes from `npm run web:mobile`. A release build without it is a
//! mistake; a check or a test only needs something to embed.

use std::path::PathBuf;

fn main() {
    let dist = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../target/web-mobile");
    if !dist.join("mobile.html").is_file() {
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!("target/web-mobile is missing: run `npm run web:mobile` first");
        }
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(
            dist.join("mobile.html"),
            "<!doctype html><title>MYLE Passwords</title><p>Run npm run web:mobile.</p>",
        )
        .unwrap();
    }
    tauri_build::build();
}
