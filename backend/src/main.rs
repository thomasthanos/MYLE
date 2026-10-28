// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    // Before anything else: a browser starting us as the password extension's host talks on stdin/stdout and never shows a window.
    if let Some(exit_code) = myle_lib::run_passwords_native_host() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = myle_lib::cleaner::run_elevated_helper_from_args() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = myle_lib::run_windows_auto_logon_helper() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = myle_lib::run_debloat_helper() {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = myle_lib::run_disk_analyzer_helper() {
        std::process::exit(exit_code);
    }
    if std::env::args().any(|argument| argument == "--game-saves-auto-backup") {
        std::process::exit(myle_lib::game_saves::run_headless_auto_backup());
    }
    // The notice a scheduled backup leaves: only its small window.
    if let Some(exit_code) = myle_lib::run_game_saves_notice() {
        std::process::exit(exit_code);
    }
    // Existing updaters and scheduled tasks may still start the old file. The installer keeps a copy there; ordinary launches switch to MYLE.exe.
    if let Ok(current) = std::env::current_exe()
        && current.file_name().is_some_and(|name| name.eq_ignore_ascii_case("MakeYourLifeEasier.exe"))
    {
        let renamed = current.with_file_name("MYLE.exe");
        if renamed.is_file()
            && std::process::Command::new(&renamed)
                .args(std::env::args_os().skip(1))
                .current_dir(renamed.parent().unwrap_or(&renamed))
                .spawn()
                .is_ok()
        {
            return;
        }
    }
    myle_lib::run()
}
