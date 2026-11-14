import AuthenticationServices
import LocalAuthentication
import Security
import SwiftRs
import Tauri
import UIKit
import WebKit
import WebKit

class SignInArgs: Decodable {
  let url: String
  let callbackScheme: String
  let ephemeral: Bool?
}

class PathArgs: Decodable {
  let path: String
}

class UnlockKeyArgs: Decodable {
  let name: String
}

class PutUnlockKeyArgs: Decodable {
  let name: String
  let key: String
}

class TakeUnlockKeyArgs: Decodable {
  let name: String
  let reason: String
  let cancelTitle: String
}

/// The Keychain item with the key that opens the vault with Face ID or
/// Touch ID (one per vault: the account is the vault's id).
private let unlockService = "uk.thomast.myle.passwords.unlock-v2"
/// Where tauri-plugin-biometry kept that key before: the iPhone passcode, or
/// a face added later, opened it too.
private let legacyUnlockService = "uk.thomast.myle.passwords.unlock"
/// Face ID's or Touch ID's enrolment when the key was made, to tell "Face ID
/// changed" from "the key is gone". Not a secret.
private let enrolmentService = "uk.thomast.myle.passwords.unlock-enrolment"

/// MYLE Passwords' own native code on iPhone: the sign-in sheet, the share
/// sheet, the key that Face ID or Touch ID opens, the cover over the app in
/// the app switcher, and keeping the app's folder out of backups.
class MyleMobilePlugin: Plugin {
  private var session: ASWebAuthenticationSession?
  /// Over the window while the app is not in front.
  private var cover: UIView?

  public override func load(webview: WKWebView) {
    let center = NotificationCenter.default
    center.addObserver(
      self, selector: #selector(coverApp), name: UIApplication.willResignActiveNotification, object: nil)
    center.addObserver(
      self, selector: #selector(uncoverApp), name: UIApplication.didBecomeActiveNotification, object: nil)
  }

  /// Leaving the front: a blur over the window before iOS takes the app
  /// switcher's picture of it (the page's own blur comes too late for that).
  @objc func coverApp() {
    guard cover == nil, let window = keyWindow() else { return }
    let blur = UIVisualEffectView(effect: UIBlurEffect(style: .systemThickMaterialDark))
    blur.frame = window.bounds
    blur.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    window.addSubview(blur)
    cover = blur
  }

  @objc func uncoverApp() {
    cover?.removeFromSuperview()
    cover = nil
  }

  /// The sign-in page in iOS's sign-in sheet, over the app. It answers with
  /// the address the page came back to (`callbackScheme`), or "cancelled".
  @objc public func signIn(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(SignInArgs.self)
    guard let url = URL(string: args.url) else {
      invoke.reject("The sign-in address is not valid.")
      return
    }
    DispatchQueue.main.async {
      self.session?.cancel()
      let session = ASWebAuthenticationSession(url: url, callbackURLScheme: args.callbackScheme) {
        [weak self] callback, error in
        self?.session = nil
        if let callback = callback {
          invoke.resolve(["callbackUrl": callback.absoluteString])
        } else if let error = error as? ASWebAuthenticationSessionError, error.code == .canceledLogin {
          invoke.reject("cancelled")
        } else {
          invoke.reject(error?.localizedDescription ?? "The sign-in did not finish.")
        }
      }
      session.presentationContextProvider = self
      // Safari's sign-ins carry over: someone already signed in to Discord
      // or Google there is not asked for their password again.
      // Private for Discord: its page, with Safari's cookies, tells Apple
      // Passwords about the account on every visit ("Passkey Updated").
      session.prefersEphemeralWebBrowserSession = args.ephemeral ?? false
      self.session = session
      if !session.start() {
        self.session = nil
        invoke.reject("The sign-in sheet could not open.")
      }
    }
  }

  /// The share sheet over one file (the Apple Passwords export). Answers
  /// `completed` once it closes: true when the user saved or sent it.
  @objc public func share(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PathArgs.self)
    let url = URL(fileURLWithPath: args.path)
    DispatchQueue.main.async {
      let sheet = UIActivityViewController(activityItems: [url], applicationActivities: nil)
      sheet.completionWithItemsHandler = { _, completed, _, error in
        if let error = error {
          invoke.reject(error.localizedDescription)
        } else {
          invoke.resolve(["completed": completed])
        }
      }
      guard let root = keyWindow()?.rootViewController else {
        invoke.reject("The share sheet could not open.")
        return
      }
      var top = root
      while let shown = top.presentedViewController { top = shown }
      // iPad: the sheet is a popover and needs an anchor.
      sheet.popoverPresentationController?.sourceView = top.view
      sheet.popoverPresentationController?.sourceRect = CGRect(x: top.view.bounds.midX, y: top.view.bounds.midY, width: 0, height: 0)
      sheet.popoverPresentationController?.permittedArrowDirections = []
      top.present(sheet, animated: true)
    }
  }

  /// Closes the sheet when the app stopped waiting for it.
  @objc public func cancelSignIn(_ invoke: Invoke) throws {
    DispatchQueue.main.async {
      self.session?.cancel()
      self.session = nil
      invoke.resolve()
    }
  }

  /// Keeps the key that opens the vault, for Face ID or Touch ID only: as
  /// enrolled now (a face or finger added later never opens it), never the
  /// passcode, never in a backup or on another iPhone.
  @objc public func putUnlockKey(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PutUnlockKeyArgs.self)
    guard
      let access = SecAccessControlCreateWithFlags(
        kCFAllocatorDefault, kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, .biometryCurrentSet, nil)
    else {
      invoke.reject("The Keychain could not protect the key.")
      return
    }
    removeUnlockItems(args.name)
    var item = itemQuery(unlockService, args.name)
    item[kSecValueData as String] = Data(args.key.utf8)
    item[kSecAttrAccessControl as String] = access
    let status = SecItemAdd(item as CFDictionary, nil)
    guard status == errSecSuccess else {
      invoke.reject("The Keychain did not keep the key (error \(status)). Check that this iPhone has a passcode.")
      return
    }
    saveEnrolment(args.name)
    invoke.resolve()
  }

  /// The key, once Face ID or Touch ID knows the user. Rejects with the code
  /// "cancelled", or "changed", "missing" or "legacy" when the key can never
  /// open the vault again.
  @objc public func takeUnlockKey(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(TakeUnlockKeyArgs.self)
    let context = LAContext()
    context.localizedReason = args.reason
    context.localizedCancelTitle = args.cancelTitle
    // No "Enter Passcode": the passcode never opens this key.
    context.localizedFallbackTitle = ""
    var query = itemQuery(unlockService, args.name)
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    query[kSecReturnData as String] = true
    query[kSecUseAuthenticationContext as String] = context
    let request = query
    // Face ID waits for the user: off the plugin's queue, so other calls go on.
    DispatchQueue.global(qos: .userInitiated).async {
      var result: CFTypeRef?
      let status = SecItemCopyMatching(request as CFDictionary, &result)
      switch status {
      case errSecSuccess:
        guard let data = result as? Data, let key = String(data: data, encoding: .utf8) else {
          invoke.reject("The key in the Keychain is damaged. Use the master password.")
          return
        }
        invoke.resolve(["key": key])
      case errSecUserCanceled:
        invoke.reject("cancelled", code: "cancelled")
      case errSecItemNotFound:
        let why = whyUnlockKeyGone(args.name)
        invoke.reject(why, code: why)
      case errSecAuthFailed:
        invoke.reject("Face ID or Touch ID did not recognise you. Use the master password.")
      default:
        invoke.reject("The Keychain did not give the key (error \(status)). Use the master password.")
      }
    }
  }

  /// Whether the key is still there, without asking for Face ID. Rejects
  /// like `takeUnlockKey` when it can never open the vault again.
  @objc public func checkUnlockKey(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(UnlockKeyArgs.self)
    if itemExists(unlockService, args.name) {
      invoke.resolve()
    } else {
      let why = whyUnlockKeyGone(args.name)
      invoke.reject(why, code: why)
    }
  }

  /// Forgets the key, the old way's too.
  @objc public func removeUnlockKey(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(UnlockKeyArgs.self)
    let status = removeUnlockItems(args.name)
    if status == errSecSuccess || status == errSecItemNotFound {
      invoke.resolve()
    } else {
      invoke.reject("The Keychain did not forget the key (error \(status)).")
    }
  }

  /// Keeps the folder at `path` (the vault, the account's session, the
  /// wrapped Face ID key) out of iCloud and computer backups. Set on the
  /// folder, it holds for the files in it, however they are replaced.
  @objc public func excludeFromBackup(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(PathArgs.self)
    var url = URL(fileURLWithPath: args.path, isDirectory: true)
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    do {
      try url.setResourceValues(values)
      invoke.resolve()
    } catch {
      invoke.reject(error.localizedDescription)
    }
  }
}

private func itemQuery(_ service: String, _ name: String) -> [String: Any] {
  return [
    kSecClass as String: kSecClassGenericPassword,
    kSecAttrService as String: service,
    kSecAttrAccount as String: name,
  ]
}

/// Whether the Keychain has the item, without asking for Face ID.
private func itemExists(_ service: String, _ name: String) -> Bool {
  let context = LAContext()
  context.interactionNotAllowed = true
  var query = itemQuery(service, name)
  query[kSecMatchLimit as String] = kSecMatchLimitOne
  query[kSecUseAuthenticationContext as String] = context
  let status = SecItemCopyMatching(query as CFDictionary, nil)
  // "Interaction not allowed": it is there, behind Face ID.
  return status == errSecSuccess || status == errSecInteractionNotAllowed
}

@discardableResult
private func removeUnlockItems(_ name: String) -> OSStatus {
  _ = SecItemDelete(itemQuery(legacyUnlockService, name) as CFDictionary)
  _ = SecItemDelete(itemQuery(enrolmentService, name) as CFDictionary)
  return SecItemDelete(itemQuery(unlockService, name) as CFDictionary)
}

/// Face ID's or Touch ID's enrolment now: it changes when a face or finger
/// is added or removed. Nil when neither is set up.
private func currentEnrolment() -> Data? {
  let context = LAContext()
  var error: NSError?
  guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
    return nil
  }
  return context.evaluatedPolicyDomainState
}

private func saveEnrolment(_ name: String) {
  guard let enrolment = currentEnrolment() else { return }
  var item = itemQuery(enrolmentService, name)
  item[kSecValueData as String] = enrolment
  item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
  _ = SecItemAdd(item as CFDictionary, nil)
}

private func savedEnrolment(_ name: String) -> Data? {
  var query = itemQuery(enrolmentService, name)
  query[kSecMatchLimit as String] = kSecMatchLimitOne
  query[kSecReturnData as String] = true
  var result: CFTypeRef?
  guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess else { return nil }
  return result as? Data
}

/// Why the key is not there: kept the old way ("legacy"), Face ID or Touch
/// ID changed since ("changed"), or gone ("missing": the passcode was turned
/// off, or the Keychain was reset).
private func whyUnlockKeyGone(_ name: String) -> String {
  if itemExists(legacyUnlockService, name) {
    return "legacy"
  }
  if let saved = savedEnrolment(name), let now = currentEnrolment(), saved != now {
    return "changed"
  }
  return "missing"
}

/// The app's window: what sheets are shown over.
func keyWindow() -> UIWindow? {
  let windows = UIApplication.shared.connectedScenes
    .compactMap { $0 as? UIWindowScene }
    .flatMap { $0.windows }
  return windows.first { $0.isKeyWindow } ?? windows.first
}

extension MyleMobilePlugin: ASWebAuthenticationPresentationContextProviding {
  func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
    return keyWindow() ?? ASPresentationAnchor()
  }
}

@_cdecl("init_plugin_myle_mobile")
func initPlugin() -> Plugin {
  return MyleMobilePlugin()
}
