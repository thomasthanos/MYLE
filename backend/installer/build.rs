//! Takes the product facts from the app's own config, so the setup can never
//! disagree with the app it installs, and points the setup binary at the
//! payload packed by `scripts/build-setup.ps1`.

use std::path::{Path, PathBuf};

/// Declares `asInvoker`: an exe called "setup" or "uninstall" must never be
/// guessed into a UAC prompt. The app installs per user, without rights.
const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
</assembly>"#;

fn main() {
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let app_config = crate_dir.join("../tauri.conf.json");
    let package_json = crate_dir.join("../../package.json");
    println!("cargo:rerun-if-changed={}", app_config.display());
    println!("cargo:rerun-if-changed={}", package_json.display());

    let config = read_json(&app_config);
    let text = |value: &serde_json::Value, what: &str| {
        value
            .as_str()
            .unwrap_or_else(|| panic!("tauri.conf.json has no {what}"))
            .to_string()
    };
    let package = read_json(&package_json);
    let version = text(&package["version"], "version in package.json");
    env("MYLE_APP_VERSION", &version);
    // Where the browser extension is downloaded; empty until it is published.
    env(
        "MYLE_EXTENSION_URL",
        package["myle"]["extensionUrl"].as_str().unwrap_or_default(),
    );
    env(
        "MYLE_PRODUCT_NAME",
        &text(&config["productName"], "productName"),
    );
    env(
        "MYLE_MAIN_BINARY",
        &text(&config["mainBinaryName"], "mainBinaryName"),
    );
    env(
        "MYLE_IDENTIFIER",
        &text(&config["identifier"], "identifier"),
    );
    env(
        "MYLE_PUBLISHER",
        &text(&config["bundle"]["publisher"], "bundle.publisher"),
    );

    // Where each bundled resource lands in the install folder. An install made
    // by the old NSIS setup has no file list, so its uninstall falls back on it.
    let resources: Vec<String> = config["bundle"]["resources"]
        .as_object()
        .map(|map| {
            map.values()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    env("MYLE_RESOURCE_FILES", &resources.join("|"));

    payload();
    frontend(&crate_dir);

    let windows = tauri_build::WindowsAttributes::new().app_manifest(MANIFEST);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run the Tauri build script");
}

/// `MYLE_PAYLOAD` names the packed app. Without one (checks, tests, the
/// uninstaller) the setup gets an empty payload and says it has nothing to install.
fn payload() {
    println!("cargo:rerun-if-env-changed=MYLE_PAYLOAD");
    let path = match std::env::var("MYLE_PAYLOAD") {
        Ok(path) if !path.trim().is_empty() => {
            let path = PathBuf::from(path.trim());
            assert!(
                path.is_file(),
                "MYLE_PAYLOAD does not name a file: {}",
                path.display()
            );
            path
        }
        _ => {
            let empty = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("no-payload.bin");
            std::fs::write(&empty, []).unwrap();
            empty
        }
    };
    println!("cargo:rerun-if-changed={}", path.display());
    env("MYLE_PAYLOAD", &path.display().to_string());
}

/// The window's page comes from `npm run web:setup`. A release build
/// without it is a mistake; a check or a test only needs something to embed.
fn frontend(crate_dir: &Path) {
    const PLACEHOLDER: &str = "<!doctype html><title>Setup</title><p>Run npm run web:setup.</p>";
    let dist = crate_dir.join("../target/web-setup");
    let page = dist.join("installer.html");
    println!("cargo:rerun-if-changed={}", page.display());
    // The stand-in a debug build left is no page either: a release never ships it.
    let real = std::fs::read_to_string(&page).is_ok_and(|text| text != PLACEHOLDER);
    if real {
        return;
    }
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        panic!("target/web-setup is missing: run `npm run web:setup` first");
    }
    if !page.is_file() {
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(&page, PLACEHOLDER).unwrap();
    }
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn env(key: &str, value: &str) {
    println!("cargo:rustc-env={key}={value}");
}
