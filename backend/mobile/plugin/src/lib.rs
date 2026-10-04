//! MYLE Passwords' own native code on phones (`android/`, `ios/`):
//!
//! - Sign-in inside the app: iOS's sign-in sheet (`ASWebAuthenticationSession`)
//!   and Android's Custom Tab, the browser's page shown over the app, with the
//!   browser's own sign-ins (Google refuses pages inside an app's webview).
//! - Installing an update (Android): the package installer over the APK the
//!   app downloaded, once the user allowed the app to install updates.
//! - Fitting the screen (Android): the page stays clear of the status bar,
//!   the navigation bar, the camera cutout and the keyboard.
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
}

#[cfg(mobile)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignInAnswer {
    #[serde(default)]
    callback_url: Option<String>,
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
    pub fn sign_in(&self, url: &str, callback_scheme: &str) -> Result<Option<String>, String> {
        self.0
            .run_mobile_plugin::<SignInAnswer>("signIn", SignInArgs { url, callback_scheme })
            .map(|answer| answer.callback_url.filter(|url| !url.is_empty()))
            .map_err(|e| e.to_string())
    }

    /// Closes the sign-in sheet when the page gave up waiting (iOS; Android's
    /// Custom Tab is the browser's to close).
    pub fn cancel_sign_in(&self) {
        #[cfg(target_os = "ios")]
        let _ = self.0.run_mobile_plugin::<serde::de::IgnoredAny>("cancelSignIn", ());
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
}

#[cfg(not(mobile))]
impl<R: Runtime> Mobile<R> {
    pub fn sign_in(&self, _url: &str, _callback_scheme: &str) -> Result<Option<String>, String> {
        Err("Signing in inside the app is for phones.".into())
    }

    pub fn cancel_sign_in(&self) {}

    pub fn install(&self, _path: &str) -> Result<(), String> {
        Err("Installing updates in the app is for Android.".into())
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
