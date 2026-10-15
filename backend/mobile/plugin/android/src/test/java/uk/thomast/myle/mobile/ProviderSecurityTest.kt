package uk.thomast.myle.mobile

import org.json.JSONArray
import org.junit.Assert.*
import org.junit.Test
import androidx.credentials.exceptions.publickeycredential.*
import androidx.credentials.exceptions.domerrors.*

class ProviderSecurityTest {
    private fun statement(pkg: String = "com.example", cert: String = "AA:BB", relation: String = "delegate_permission/common.get_login_creds") =
        JSONArray("""[{"relation":["$relation"],"target":{"namespace":"android_app","package_name":"$pkg","sha256_cert_fingerprints":["$cert"]}}]""")
    @Test fun assetLinksRequiresExactPackageCertificateAndLoginRelation() {
        assertTrue(AssetLinks.matches(statement(), "com.example", "AA:BB"))
        assertFalse(AssetLinks.matches(statement(pkg = "com.example.evil"), "com.example", "AA:BB"))
        assertFalse(AssetLinks.matches(statement(cert = "CC:DD"), "com.example", "AA:BB"))
        assertFalse(AssetLinks.matches(statement(relation = "delegate_permission/common.handle_all_urls"), "com.example", "AA:BB"))
        assertFalse(AssetLinks.matches(JSONArray("[]"), "com.example", "AA:BB"))
    }
    @Test fun unknownAndMalformedAssetLinksDoNotAuthorizeApps() {
        assertFalse(AssetLinks.matches(JSONArray("""[{"target":{"namespace":"web","site":"https://example.com"}},{"relation":[],"target":{}}]"""), "com.example", "AA:BB"))
    }
    @Test fun publicSuffixCannotBeUsedAsRelyingParty() {
        assertFalse(CredentialPolicy.rpMatches("https://login.example.co.uk", "co.uk"))
        assertFalse(CredentialPolicy.rpMatches("https://user.github.io", "github.io"))
        assertTrue(CredentialPolicy.rpMatches("https://login.example.co.uk", "example.co.uk"))
    }
    @Test fun onlyTrustedCredentialManagerBrowserMayUseRelatedOrigins() {
        assertEquals("example.co.uk", CredentialPolicy.credentialBrowserSite("https://example.com", "example.co.uk", true))
        assertEquals("example.com", CredentialPolicy.credentialBrowserSite("https://example.com", null, true))
        assertTrue(runCatching { CredentialPolicy.credentialBrowserSite("https://example.com", "example.co.uk", false) }.isFailure)
        assertTrue(runCatching { CredentialPolicy.credentialBrowserSite("https://example.com", "co.uk", true) }.isFailure)
        assertTrue(runCatching { CredentialPolicy.credentialBrowserSite("http://example.com", "example.co.uk", true) }.isFailure)
        assertFalse(CredentialPolicy.rpMatches("https://example.com", "example.co.uk"))
    }
    @Test fun anyWebsiteClaimPreventsFallbackToUnclaimedNativeFields() {
        assertTrue(CredentialPolicy.consistentFormDomains(setOf(null), emptySet()))
        assertTrue(CredentialPolicy.consistentFormDomains(setOf("example.com"), setOf("example.com")))
        assertFalse(CredentialPolicy.consistentFormDomains(setOf(null), setOf("example.com")))
        assertFalse(CredentialPolicy.consistentFormDomains(setOf("example.com"), setOf("example.com", "evil.com")))
        assertFalse(CredentialPolicy.consistentFormDomains(setOf(null, "example.com"), setOf("example.com")))
    }
    @Test fun rootWebauthnFailuresAreReturnedAsDomExceptions() {
        assertTrue((ProviderErrors.create("excluded") as CreatePublicKeyCredentialDomException).domError is InvalidStateError)
        assertTrue((ProviderErrors.create("notSupported") as CreatePublicKeyCredentialDomException).domError is NotSupportedError)
        assertTrue((ProviderErrors.get("badRequest") as GetPublicKeyCredentialDomException).domError is DataError)
        assertTrue((ProviderErrors.get("rpMismatch") as GetPublicKeyCredentialDomException).domError is SecurityError)
        assertTrue((ProviderErrors.get("noCredential") as GetPublicKeyCredentialDomException).domError is NotAllowedError)
    }
}
