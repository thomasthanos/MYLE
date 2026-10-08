fn main() {
    bundle_release_notes();
    tauri_build::build()
}

/// This version's notes (`docs/release-notes/<version>.md`), for the "What's
/// new" dialog when GitHub can't be reached; empty when there are none.
fn bundle_release_notes() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let path = std::path::Path::new("../docs/release-notes").join(format!("{version}.md"));
    println!("cargo:rerun-if-changed={}", path.display());
    let notes = std::fs::read_to_string(&path).unwrap_or_default();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("release-notes.md"), notes).expect("write the bundled release notes");
}
