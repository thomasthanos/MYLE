//! MYLE Passwords: the Password Manager on phones (Android and iOS), over
//! the same vault and account as the Windows app (`myle-vault`). It shows
//! and edits the logins and keeps them in sync with the PC; it does not fill
//! them into other apps.

mod account;
mod clipboard;
mod passwords;
mod unlock;
mod update;

use tauri::Manager;

/// Android's back button on the first screen: the app closes, and the vault
/// with it.
#[tauri::command]
fn mobile_leave(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(mobile)]
    let builder = builder
        .plugin(tauri_plugin_barcode_scanner::init())
        .plugin(tauri_plugin_biometry::init());
    builder
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let passwords = passwords::PasswordsState::new(dir.clone());
            app.manage(account::AccountState::new(dir));
            app.manage(passwords.clone());
            account::listen(app.handle());
            passwords::watch(app.handle().clone(), passwords);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            account::account_profile,
            account::account_sign_in,
            account::account_cancel_sign_in,
            account::account_sign_out,
            passwords::passwords_status,
            passwords::passwords_app_hidden,
            passwords::passwords_create,
            passwords::passwords_unlock,
            passwords::passwords_recover,
            passwords::passwords_change_master,
            passwords::passwords_sync,
            passwords::passwords_use_account_vault,
            passwords::passwords_lock,
            passwords::passwords_set_auto_lock,
            passwords::passwords_set_website_icons,
            passwords::passwords_icons,
            passwords::passwords_list,
            passwords::passwords_reveal,
            passwords::passwords_history,
            passwords::passwords_copy,
            passwords::passwords_copy_text,
            passwords::passwords_totp,
            passwords::passwords_totp_check,
            passwords::passwords_passkey_delete,
            passwords::passwords_save,
            passwords::passwords_delete,
            passwords::passwords_generate,
            passwords::passwords_strength,
            unlock::passwords_hello_status,
            unlock::passwords_hello_enable,
            unlock::passwords_hello_disable,
            unlock::passwords_hello_unlock,
            update::mobile_update_check,
            mobile_leave,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MYLE Passwords");
}
