# Android app integration

The native plugin provides account sign-in through a Custom Tab, APK update installation, and screen insets. MYLE's webview excludes its vault fields from other apps' autofill services.

Passwords are viewed and copied manually inside MYLE. TOTP codes are generated in the vault and displayed in the app, including their countdown and copy action; the QR scanner still adds 2FA keys. Account sync and biometric vault unlock remain available.

There are no Android Autofill or Credential Manager services, credential activities, JNI vault bridge, provider settings, or provider-specific dependencies. Passkeys already stored by the desktop app remain in the encrypted vault for sync and desktop use.

The mobile workflow builds the APK and assembles the native plugin, and checks that the merged manifest disables Android backup and contains no credential or autofill service.
