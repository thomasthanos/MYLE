// Phones start the app from the library (`run`); this is the Windows build,
// for trying the app there.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    myle_passwords_lib::run()
}
