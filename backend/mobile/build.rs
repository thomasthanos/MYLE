//! The page comes from `npm run web:mobile`. A release build without it is a
//! mistake; a check or a test only needs something to embed.

use std::path::PathBuf;

const PLACEHOLDER: &str = "<!doctype html><title>MYLE Passwords</title><p>Run npm run web:mobile.</p>";

fn main() {
    let dist = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../target/web-mobile");
    let page = dist.join("mobile.html");
    println!("cargo:rerun-if-changed={}", page.display());
    // The stand-in a debug build left is no page either: a release never ships it.
    let real = std::fs::read_to_string(&page).is_ok_and(|text| text != PLACEHOLDER);
    if !real {
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            panic!("target/web-mobile is missing: run `npm run web:mobile` first");
        }
        if !page.is_file() {
            std::fs::create_dir_all(&dist).unwrap();
            std::fs::write(&page, PLACEHOLDER).unwrap();
        }
    }
    tauri_build::build();
}
