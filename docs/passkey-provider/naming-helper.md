# Naming a new passkey MYLE

MYLE 10.0.4 offers naming assistance without submitting a third-party provider registration.

1. Choose **Save in MYLE** when creating a passkey.
2. If the website opens a supported naming dialog on the same page, MYLE fills its name field with **MYLE**.
3. Confirm the website's **Save** button. You can change the suggested name first.

The extension watches for two minutes after MYLE returns the credential. It accepts an explicit **new passkey** naming prompt, or a naming editor carrying that credential's matching `data-credential-id`. Generic name editors without credential identity, old inputs, hidden fields, ambiguous forms and inputs you edited are skipped. English and Greek prompt wording is supported. Full page navigation ends assistance.

This changes a website's editable label only. Websites determine whether they offer a naming field and how they identify the provider; WebAuthn has no universal field through which MYLE can force its provider name. Credential IDs, account names, AAGUID, cryptographic keys and registration responses remain unchanged. A site may still show an automatically assigned name if its interface cannot be identified safely.

The tests cover DOM fixtures and the MYLE creation integration. Google Account's current interface has not been verified because account reauthentication was unavailable. If the helper does not fill the label, use the site's own rename action where available.

Reference: <a href="https://developers.google.com/identity/passkeys/developer-guides/server-registration"><img alt="Google's server-side registration guide" src="https://img.shields.io/static/v1?label=&message=Google's%20server-side%20registration%20guide&color=8b83ff&labelColor=151c31&style=flat-square" /></a> documents the optional credential name stored by a relying party.
