// Builds the Kotlin (android/) and Swift (ios/) halves into the phone apps.
// The page never calls this plugin: only the app's Rust does, so it has no
// commands of its own to allow.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
