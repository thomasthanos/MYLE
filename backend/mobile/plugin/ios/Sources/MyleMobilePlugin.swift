import AuthenticationServices
import SwiftRs
import Tauri
import UIKit
import WebKit

class SignInArgs: Decodable {
  let url: String
  let callbackScheme: String
}

class ShareArgs: Decodable {
  let path: String
}

/// MYLE Passwords' own native code on iPhone: the sign-in sheet.
class MyleMobilePlugin: Plugin {
  private var session: ASWebAuthenticationSession?

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
      session.prefersEphemeralWebBrowserSession = false
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
    let args = try invoke.parseArgs(ShareArgs.self)
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
