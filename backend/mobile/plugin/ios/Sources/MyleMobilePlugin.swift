import AuthenticationServices
import SwiftRs
import Tauri
import UIKit
import WebKit

class SignInArgs: Decodable {
  let url: String
  let callbackScheme: String
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

  /// Closes the sheet when the app stopped waiting for it.
  @objc public func cancelSignIn(_ invoke: Invoke) throws {
    DispatchQueue.main.async {
      self.session?.cancel()
      self.session = nil
      invoke.resolve()
    }
  }
}

extension MyleMobilePlugin: ASWebAuthenticationPresentationContextProviding {
  func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
    let windows = UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap { $0.windows }
    return windows.first { $0.isKeyWindow } ?? windows.first ?? ASPresentationAnchor()
  }
}

@_cdecl("init_plugin_myle_mobile")
func initPlugin() -> Plugin {
  return MyleMobilePlugin()
}
