# MYLE passkey provider registration

Add the entry from `aaguid-entry.json` to `aaguid.json` in
https://github.com/passkeydeveloper/passkey-authenticator-aaguids.

MYLE uses AAGUID `1afc30cd-b888-46ee-8d17-ef0f4e5e5089` in the attested
credential data of newly created passkeys. The fixed identifier is defined
in `backend/vault/src/passkeys.rs` and already ships in MYLE 10.0.3.

Relying parties decide the name shown on their passkey management pages.
Registration lets sites using this provider list map the existing AAGUID
to **MYLE**. It does not guarantee adoption by Google or every other site,
and does not change existing credentials, account names, keys or signatures.

## Proposed pull request

Title: Add MYLE passkey provider AAGUID

Body:

Adds MYLE's existing AAGUID with the friendly provider name `MYLE`, so
relying parties using this list can identify passkeys created by MYLE.

- Provider: https://github.com/thomasthanos/MYLE
- Implementation: https://github.com/thomasthanos/MYLE/blob/main/backend/vault/src/passkeys.rs
- AAGUID: `1afc30cd-b888-46ee-8d17-ef0f4e5e5089`
- Name: `MYLE`

The metadata entry matches the implementation's AAGUID bytes and the
upstream JSON schema. This entry is for display naming only, not an
attestation or security certification.

## Submission

Submitting the external pull request requires explicit user authorization.
The upstream project may also request verification of the provider owner.
After acceptance, adoption and refresh timing are controlled by each site.

Sources:

- https://github.com/passkeydeveloper/passkey-authenticator-aaguids
- https://developers.google.com/identity/passkeys/developer-guides/server-registration
