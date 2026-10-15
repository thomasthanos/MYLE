//! Synthetic public responses for an independent relying-party verifier.
//! No private key or real account data leaves this process.
use myle_vault::passkeys::{self, ClientContext, User};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn main() {
    let mut vectors = Vec::new();
    let apk_origin = format!("android:apk-key-hash:{}", passkeys::b64(&[3;32]));
    for (name, origin, package, browser_hash) in [
        ("desktop", "https://example.com", None, false),
        ("https-port", "https://login.example.com:8443", None, false),
        ("android-app", apk_origin.as_str(), Some("com.example.app"), false),
        ("android-browser-related-origin", "https://example.net", None, true),
    ] {
        let register_challenge = passkeys::b64(b"independent registration challenge");
        let login_challenge = passkeys::b64(b"independent assertion challenge");
        let client = |kind: &str, challenge: &str| json!({"type":kind,"challenge":challenge,"origin":origin,"crossOrigin":false}).to_string();
        let register_json = client("webauthn.create", &register_challenge);
        let login_json = client("webauthn.get", &login_challenge);
        let register_hash = Sha256::digest(register_json.as_bytes());
        let login_hash = Sha256::digest(login_json.as_bytes());
        let context = ClientContext { origin, android_package: package, hash: browser_hash.then_some(register_hash.as_slice()) };
        let user = User { handle:passkeys::b64(b"synthetic account"), name:"fixture@example.com".into(), display_name:"Fixture".into() };
        let (key, mut registration) = passkeys::create_with_context("example.com", "Example", &user, &register_challenge, &context, true, 1).unwrap();
        let context = ClientContext { origin, android_package: package, hash:browser_hash.then_some(login_hash.as_slice()) };
        let mut authentication = passkeys::sign_in_with_context(&key, &login_challenge, &context, true).unwrap();
        let mut unverified = passkeys::sign_in_with_context(&key, &login_challenge, &context, false).unwrap();
        if browser_hash {
            // Android's browser attaches the original client data before
            // sending the response to the RP, not the credential provider.
            registration["response"]["clientDataJSON"] = json!(passkeys::b64(register_json.as_bytes()));
            for response in [&mut authentication, &mut unverified] {
                response["response"]["clientDataJSON"] = json!(passkeys::b64(login_json.as_bytes()));
            }
        }
        for response in [&mut registration, &mut authentication, &mut unverified] {
            response["clientExtensionResults"] = json!({});
        }
        vectors.push(json!({"name":name,"origin":origin,"rpId":"example.com","registrationChallenge":register_challenge,"authenticationChallenge":login_challenge,"registration":registration,"authentication":authentication,"unverified":unverified}));
    }
    println!("{}", Value::Array(vectors));
}
