//! MYLE's release certificate, "ThomasT" (`scripts/new-release-certificate.ps1`),
//! which signs the app, the uninstaller and the setup. It is MYLE's own: no
//! public authority issued it, so Windows names no publisher for MYLE until
//! the user trusts it. The setup's last page offers that, for this Windows
//! user only, with no administrator rights: Windows itself asks before it adds
//! a root certificate, and again before the uninstaller takes it away. The
//! certificate is not a certificate authority and signs programs only, so
//! trusting it vouches for nothing else.
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CRYPT_E_EXISTS, ERROR_CANCELLED, GetLastError};
use windows_sys::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_FIND_EXISTING, CERT_STORE_ADD_NEW, CertAddCertificateContextToStore,
    CertCloseStore, CertCreateCertificateContext, CertDeleteCertificateFromStore, CertFindCertificateInStore,
    CertFreeCertificateContext, CertOpenSystemStoreW, HCERTSTORE, X509_ASN_ENCODING,
};

use crate::shell;

/// The name Windows shows as MYLE's publisher once the user trusts it.
pub const NAME: &str = "ThomasT";

/// The public part only: the key stays in the release secret.
const CERTIFICATE: &[u8] = include_bytes!("../ThomasT.cer");

/// This user's trusted root certificates (a view that also holds the
/// machine's).
struct Roots(HCERTSTORE);

impl Roots {
    fn open() -> Option<Self> {
        let name = shell::wide("ROOT");
        // SAFETY: `name` is NUL-terminated; the store is closed on drop.
        let store = unsafe { CertOpenSystemStoreW(0, name.as_ptr()) };
        (!store.is_null()).then_some(Self(store))
    }

    /// This store's copy of `certificate`, when it has one.
    fn find(&self, certificate: &Certificate) -> Option<Certificate> {
        // SAFETY: both are open; the copy found is freed on drop.
        let found = unsafe {
            CertFindCertificateInStore(self.0, X509_ASN_ENCODING, 0, CERT_FIND_EXISTING, certificate.0.cast(), null())
        };
        (!found.is_null()).then_some(Certificate(found))
    }
}

impl Drop for Roots {
    fn drop(&mut self) {
        // SAFETY: opened by `open`.
        unsafe { CertCloseStore(self.0, 0) };
    }
}

struct Certificate(*const CERT_CONTEXT);

impl Certificate {
    /// The certificate this setup carries.
    fn ours() -> Option<Self> {
        // SAFETY: reads `CERTIFICATE` only; the context is freed on drop.
        let context =
            unsafe { CertCreateCertificateContext(X509_ASN_ENCODING, CERTIFICATE.as_ptr(), CERTIFICATE.len() as u32) };
        (!context.is_null()).then_some(Self(context))
    }
}

impl Drop for Certificate {
    fn drop(&mut self) {
        // SAFETY: a context this module created or found, freed once.
        unsafe { CertFreeCertificateContext(self.0) };
    }
}

/// Whether Windows already trusts the certificate on this PC, for this user
/// or for every user.
pub fn is_trusted() -> bool {
    let (Some(roots), Some(ours)) = (Roots::open(), Certificate::ours()) else {
        return false;
    };
    roots.find(&ours).is_some()
}

/// Trusts the certificate for this user. Windows asks the user first, with a
/// window of its own; `Ok(false)` when they said no. Blocks until they answer.
pub fn trust() -> Result<bool, String> {
    let roots = Roots::open().ok_or("Windows did not open the trusted certificates.")?;
    let ours = Certificate::ours().ok_or("The certificate in this setup is damaged.")?;
    // SAFETY: both are open; no copy is asked back.
    let added = unsafe { CertAddCertificateContextToStore(roots.0, ours.0, CERT_STORE_ADD_NEW, null_mut()) } != 0;
    // SAFETY: read right after the call it describes.
    let code = unsafe { GetLastError() };
    if added || code == CRYPT_E_EXISTS as u32 {
        Ok(true)
    } else if code == ERROR_CANCELLED {
        Ok(false)
    } else {
        Err(format!("Windows did not add the certificate (0x{code:08X})."))
    }
}

/// Takes back the trust `trust` gave, when there is any; Windows asks the user
/// first. Trust an administrator gave every user stays: that is theirs.
pub fn forget() {
    let (Some(roots), Some(ours)) = (Roots::open(), Certificate::ours()) else {
        return;
    };
    if let Some(found) = roots.find(&ours) {
        // SAFETY: deleting frees the context, whether or not it succeeds, so
        // `found` must not be freed again.
        unsafe { CertDeleteCertificateFromStore(found.0) };
        std::mem::forget(found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Cryptography::{
        CERT_BASIC_CONSTRAINTS2_INFO, CERT_NAME_SIMPLE_DISPLAY_TYPE, CTL_USAGE, CertFindExtension,
        CertGetEnhancedKeyUsage, CertGetNameStringW, CryptDecodeObject, X509_BASIC_CONSTRAINTS2,
        szOID_BASIC_CONSTRAINTS2,
    };

    // `trust` and `forget` are never called here: they would change this
    // PC's trusted certificates (after Windows' question).

    /// What the setup offers to trust is ThomasT's certificate, made for
    /// signing programs and nothing else, and not a certificate authority.
    #[test]
    fn the_certificate_is_thomast_s_and_signs_programs_only() {
        let ours = Certificate::ours().expect("backend/installer/ThomasT.cer is a certificate");
        // SAFETY: `ours` stays alive for every call below; buffers are sized
        // by the calls themselves.
        unsafe {
            let mut name = [0u16; 128];
            let length = CertGetNameStringW(ours.0, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, null(), name.as_mut_ptr(), 128);
            assert_eq!(String::from_utf16_lossy(&name[..length.saturating_sub(1) as usize]), NAME);

            let info = &*(*ours.0).pCertInfo;
            let extension = CertFindExtension(szOID_BASIC_CONSTRAINTS2, info.cExtension, info.rgExtension);
            assert!(!extension.is_null(), "it says whether it is a certificate authority");
            let mut constraints = CERT_BASIC_CONSTRAINTS2_INFO::default();
            let mut size = size_of::<CERT_BASIC_CONSTRAINTS2_INFO>() as u32;
            assert_ne!(
                CryptDecodeObject(
                    X509_ASN_ENCODING,
                    X509_BASIC_CONSTRAINTS2,
                    (*extension).Value.pbData,
                    (*extension).Value.cbData,
                    0,
                    (&raw mut constraints).cast(),
                    &mut size,
                ),
                0
            );
            assert_eq!(constraints.fCA, 0, "not a certificate authority");

            let mut size = 0u32;
            assert_ne!(CertGetEnhancedKeyUsage(ours.0, 0, null_mut(), &mut size), 0);
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let usage = buffer.as_mut_ptr().cast::<CTL_USAGE>();
            assert_ne!(CertGetEnhancedKeyUsage(ours.0, 0, usage, &mut size), 0);
            let purposes: Vec<String> = (0..(*usage).cUsageIdentifier as usize)
                .map(|i| {
                    std::ffi::CStr::from_ptr((*(*usage).rgpszUsageIdentifier.add(i)).cast())
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(purposes, ["1.3.6.1.5.5.7.3.3"], "code signing only");
        }
    }
}
