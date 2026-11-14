//! MYLE Passwords' own native code on phones (`android/`, `ios/`):
//!
//! - Sign-in inside the app: iOS's sign-in sheet (`ASWebAuthenticationSession`)
//!   and Android's Custom Tab, the browser's page shown over the app, with the
//!   browser's own sign-ins (Google refuses pages inside an app's webview).
//! - Installing an update (Android): the package installer over the APK the
//!   app downloaded, once the user allowed the app to install updates.
//! - Fitting the screen (Android): the page stays clear of the status bar,
//!   the navigation bar, the camera cutout and the keyboard.
//! - The key that Face ID or Touch ID opens (iOS): in the Keychain, for the
//!   faces and fingers enrolled when it was made, never the passcode.
//! - Privacy (iOS): a blur over the app in the app switcher, and the app's
//!   folder kept out of iCloud and computer backups.
//!
//! On Windows (where the app is built to try it) it does nothing.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_myle_mobile);

/// What `install` answers when Android first needs the user to allow this
/// app to install updates: Android's setting is open, Install again after.
pub const ALLOW_INSTALLS: &str = "allow-installs";
/// What `sign_in` answers when the user closed the sign-in sheet.
pub const CANCELLED: &str = "cancelled";
#[cfg(not(target_os = "ios"))]
const FOR_IPHONE: &str = "Face ID and Touch ID are for iPhone.";

/// Why the key that Face ID or Touch ID opens did not come back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The user closed the prompt.
    Cancelled,
    /// The key can never open the vault again.
    Gone(KeyGone),
    /// Anything else, for the user to read.
    Failed(String),
}

/// Why the key can never open the vault again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyGone {
    /// A face or finger was added or removed since it was made.
    Changed,
    /// It is not in the Keychain: the passcode was turned off, or the
    /// Keychain was reset.
    Missing,
    /// It was kept the old way (tauri-plugin-biometry's), which the passcode
    /// also opened. It is forgotten; the user turns it on again.
    Legacy,
}

impl KeyError {
    /// The Swift side's answer: its code says which.
    #[cfg(any(target_os = "ios", test))]
    fn from_reply(code: Option<&str>, message: Option<String>) -> Self {
        match code {
            Some(CANCELLED) => Self::Cancelled,
            Some("changed") => Self::Gone(KeyGone::Changed),
            Some("missing") => Self::Gone(KeyGone::Missing),
            Some("legacy") => Self::Gone(KeyGone::Legacy),
            _ => Self::Failed(message.unwrap_or_else(|| "The Keychain did not answer.".into())),
        }
    }
}

#[cfg(target_os = "ios")]
fn key_error(error: tauri::plugin::mobile::PluginInvokeError) -> KeyError {
    match error {
        tauri::plugin::mobile::PluginInvokeError::InvokeRejected(reply) => {
            KeyError::from_reply(reply.code.as_deref(), reply.message)
        }
        other => KeyError::Failed(other.to_string()),
    }
}

#[cfg(mobile)]
pub struct Mobile<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(not(mobile))]
pub struct Mobile<R: Runtime>(std::marker::PhantomData<fn() -> R>);

#[cfg(mobile)]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInArgs<'a> {
    url: &'a str,
    callback_scheme: &'a str,
    /// iOS: a private sheet, without Safari's cookies.
    ephemeral: bool,
}

#[cfg(mobile)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignInAnswer {
    #[serde(default)]
    callback_url: Option<String>,
}

#[cfg(target_os = "ios")]
#[derive(serde::Serialize)]
struct PathArgs<'a> {
    path: &'a str,
}

#[cfg(target_os = "ios")]
#[derive(serde::Serialize)]
struct UnlockKeyArgs<'a> {
    name: &'a str,
}

#[cfg(target_os = "ios")]
#[derive(serde::Serialize)]
struct PutUnlockKeyArgs<'a> {
    name: &'a str,
    key: &'a str,
}

#[cfg(target_os = "ios")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TakeUnlockKeyArgs<'a> {
    name: &'a str,
    reason: &'a str,
    cancel_title: &'a str,
}

#[cfg(target_os = "ios")]
#[derive(serde::Deserialize)]
struct UnlockKeyAnswer {
    key: String,
}

#[cfg(target_os = "ios")]
#[derive(serde::Deserialize)]
struct ShareAnswer {
    #[serde(default)]
    completed: bool,
}

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
struct InstallArgs<'a> {
    path: &'a str,
}

#[cfg(mobile)]
impl<R: Runtime> Mobile<R> {
    /// Shows the sign-in page `url` inside the app. On iOS this waits for
    /// the page to come back to `callback_scheme` and answers with that
    /// address (an error with `CANCELLED` when the user closed it); on
    /// Android it answers `None` at once, and the page comes back as a link
    /// to the app. Blocks: call it off the async runtime.
    pub fn sign_in(&self, url: &str, callback_scheme: &str, ephemeral: bool) -> Result<Option<String>, String> {
        self.0
            .run_mobile_plugin::<SignInAnswer>("signIn", SignInArgs { url, callback_scheme, ephemeral })
            .map(|answer| answer.callback_url.filter(|url| !url.is_empty()))
            .map_err(|e| e.to_string())
    }

    /// Closes the sign-in sheet when the page gave up waiting (iOS; Android's
    /// Custom Tab is the browser's to close).
    pub fn cancel_sign_in(&self) {
        #[cfg(target_os = "ios")]
        let _ = self.0.run_mobile_plugin::<serde::de::IgnoredAny>("cancelSignIn", ());
    }

    /// iOS's share sheet over the file at `path`; waits until it closes and
    /// answers whether the user sent or saved it. Blocks.
    pub fn share(&self, path: &str) -> Result<bool, String> {
        #[cfg(target_os = "ios")]
        return self
            .0
            .run_mobile_plugin::<ShareAnswer>("share", PathArgs { path })
            .map(|answer| answer.completed)
            .map_err(|e| e.to_string());
        #[cfg(target_os = "android")]
        {
            let _ = path;
            Err("Exporting for Apple Passwords is for iPhone.".into())
        }
    }

    /// Opens Android's package installer over the APK at `path`, or answers
    /// an error with `ALLOW_INSTALLS` after showing the setting that allows it.
    pub fn install(&self, path: &str) -> Result<(), String> {
        #[cfg(target_os = "android")]
        return self
            .0
            .run_mobile_plugin::<serde::de::IgnoredAny>("install", InstallArgs { path })
            .map(|_| ())
            .map_err(|e| e.to_string());
        #[cfg(target_os = "ios")]
        {
            let _ = path;
            Err("iPhones get updates through SideStore.".into())
        }
    }

    /// Whether the Keychain still has the key that opens the vault `name`,
    /// without asking for Face ID (iOS). `Gone` when it never will again.
    pub fn check_unlock_key(&self, name: &str) -> Result<(), KeyError> {
        #[cfg(target_os = "ios")]
        return self
            .0
            .run_mobile_plugin::<serde::de::IgnoredAny>("checkUnlockKey", UnlockKeyArgs { name })
            .map(|_| ())
            .map_err(key_error);
        #[cfg(target_os = "android")]
        {
            let _ = name;
            Err(KeyError::Failed(FOR_IPHONE.into()))
        }
    }

    /// Keeps `key` in the Keychain for the vault `name`, opened by Face ID or
    /// Touch ID only, as enrolled now (iOS).
    pub fn put_unlock_key(&self, name: &str, key: &str) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        return self
            .0
            .run_mobile_plugin::<serde::de::IgnoredAny>("putUnlockKey", PutUnlockKeyArgs { name, key })
            .map(|_| ())
            .map_err(|e| key_error(e).to_string());
        #[cfg(target_os = "android")]
        {
            let _ = (name, key);
            Err(FOR_IPHONE.into())
        }
    }

    /// The key for the vault `name`, once Face ID or Touch ID knows the user
    /// (iOS). Blocks while they look or touch: call it off the async runtime.
    pub fn take_unlock_key(&self, name: &str, reason: &str, cancel_title: &str) -> Result<String, KeyError> {
        #[cfg(target_os = "ios")]
        return self
            .0
            .run_mobile_plugin::<UnlockKeyAnswer>("takeUnlockKey", TakeUnlockKeyArgs { name, reason, cancel_title })
            .map(|answer| answer.key)
            .map_err(key_error);
        #[cfg(target_os = "android")]
        {
            let _ = (name, reason, cancel_title);
            Err(KeyError::Failed(FOR_IPHONE.into()))
        }
    }

    /// Forgets the key for the vault `name` (iOS), kept the old way or the new.
    pub fn remove_unlock_key(&self, name: &str) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        return self
            .0
            .run_mobile_plugin::<serde::de::IgnoredAny>("removeUnlockKey", UnlockKeyArgs { name })
            .map(|_| ())
            .map_err(|e| key_error(e).to_string());
        #[cfg(target_os = "android")]
        {
            let _ = name;
            Ok(())
        }
    }

    /// Keeps the folder at `path` out of iCloud and computer backups (iOS;
    /// Android's manifest rules do it there).
    pub fn exclude_from_backup(&self, path: &str) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        return self
            .0
            .run_mobile_plugin::<serde::de::IgnoredAny>("excludeFromBackup", PathArgs { path })
            .map(|_| ())
            .map_err(|e| e.to_string());
        #[cfg(target_os = "android")]
        {
            let _ = path;
            Ok(())
        }
    }
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Cancelled."),
            Self::Gone(_) => f.write_str("The key that Face ID or Touch ID opens is gone."),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

#[cfg(not(mobile))]
impl<R: Runtime> Mobile<R> {
    pub fn sign_in(&self, _url: &str, _callback_scheme: &str, _ephemeral: bool) -> Result<Option<String>, String> {
        Err("Signing in inside the app is for phones.".into())
    }

    pub fn cancel_sign_in(&self) {}

    pub fn share(&self, _path: &str) -> Result<bool, String> {
        Err("Exporting for Apple Passwords is for iPhone.".into())
    }

    pub fn install(&self, _path: &str) -> Result<(), String> {
        Err("Installing updates in the app is for Android.".into())
    }

    pub fn check_unlock_key(&self, _name: &str) -> Result<(), KeyError> {
        Err(KeyError::Failed(FOR_IPHONE.into()))
    }

    pub fn put_unlock_key(&self, _name: &str, _key: &str) -> Result<(), String> {
        Err(FOR_IPHONE.into())
    }

    pub fn take_unlock_key(&self, _name: &str, _reason: &str, _cancel_title: &str) -> Result<String, KeyError> {
        Err(KeyError::Failed(FOR_IPHONE.into()))
    }

    /// Nothing was kept, so there is nothing to forget.
    pub fn remove_unlock_key(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }

    /// Windows' backups are the user's own business.
    pub fn exclude_from_backup(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
}

pub trait MobileExt<R: Runtime> {
    fn myle_mobile(&self) -> &Mobile<R>;
}

impl<R: Runtime, T: Manager<R>> MobileExt<R> for T {
    fn myle_mobile(&self) -> &Mobile<R> {
        self.state::<Mobile<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("myle-mobile")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let mobile = Mobile(api.register_android_plugin("uk.thomast.myle.mobile", "MyleMobilePlugin")?);
            #[cfg(target_os = "ios")]
            let mobile = Mobile(api.register_ios_plugin(init_plugin_myle_mobile)?);
            #[cfg(not(mobile))]
            let mobile = {
                let _ = api;
                Mobile::<R>(std::marker::PhantomData)
            };
            app.manage(mobile);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keychain_answer_says_whether_the_key_is_gone_for_good() {
        let reply = |code: Option<&str>| KeyError::from_reply(code, Some("The Keychain said no.".into()));
        assert_eq!(reply(Some("cancelled")), KeyError::Cancelled);
        assert_eq!(reply(Some("changed")), KeyError::Gone(KeyGone::Changed));
        assert_eq!(reply(Some("missing")), KeyError::Gone(KeyGone::Missing));
        assert_eq!(reply(Some("legacy")), KeyError::Gone(KeyGone::Legacy));
        // Anything else is a failure to show, never a reason to forget the key.
        assert_eq!(reply(None), KeyError::Failed("The Keychain said no.".into()));
        assert_eq!(reply(Some("userCancel")), KeyError::Failed("The Keychain said no.".into()));
        assert!(matches!(KeyError::from_reply(None, None), KeyError::Failed(message) if !message.is_empty()));
    }
}
