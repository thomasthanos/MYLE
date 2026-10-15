package uk.thomast.myle.mobile

import org.junit.Assert.*
import org.junit.Test

class CredentialPolicyTest {
    @Test fun ownAppCannotStartAnAutofillOrSaveRequest() {
        assertNull(CredentialPolicy.autofillRequestPackage("uk.thomast.myle", "uk.thomast.myle"))
        // Use the installed application ID so development variants are excluded too.
        assertNull(CredentialPolicy.autofillRequestPackage("uk.thomast.myle.debug", "uk.thomast.myle.debug"))
    }
    @Test fun excludingOwnAppPreservesExactExternalPackageIdentity() {
        assertEquals("com.android.chrome", CredentialPolicy.autofillRequestPackage("com.android.chrome", "uk.thomast.myle"))
        assertEquals("uk.thomast.myle.other", CredentialPolicy.autofillRequestPackage("uk.thomast.myle.other", "uk.thomast.myle"))
        assertNull(CredentialPolicy.autofillRequestPackage(null, "uk.thomast.myle"))
    }
    @Test fun validHttpsOriginsMayUseNondefaultPorts() {
        assertEquals("example.com", CredentialPolicy.originHost("https://example.com:8443"))
        assertTrue(CredentialPolicy.rpMatches("https://login.example.com:8443", "example.com"))
        assertEquals("example.com", CredentialPolicy.credentialBrowserSite("https://example.com:8443", null, true))
        assertTrue(runCatching { CredentialPolicy.originHost("https://example.com:65536") }.isFailure)
        assertTrue(runCatching { CredentialPolicy.originHost("https://example.com:invalid") }.isFailure)
        assertTrue(runCatching { CredentialPolicy.originHost("http://example.com:8443") }.isFailure)
        assertTrue(runCatching { CredentialPolicy.originHost("https://alice@example.com:8443") }.isFailure)
        assertTrue(runCatching { CredentialPolicy.originHost("https://example.com:8443/path") }.isFailure)
        assertTrue(runCatching { CredentialPolicy.originHost("https://example.com:8443#fragment") }.isFailure)
    }
    @Test fun credentialManagerPasswordRequiresAStoredPasswordAndItsActualUsername() {
        assertTrue(CredentialPolicy.passwordCredentialEligible(true, "alice"))
        assertFalse(CredentialPolicy.passwordCredentialEligible(false, "alice"))
        assertFalse(CredentialPolicy.passwordCredentialEligible(true, ""))
        assertFalse(CredentialPolicy.passwordCredentialEligible(false, ""))
    }
    @Test fun autofillRequiresEveryRequestedSecretCapability() {
        assertTrue(CredentialPolicy.autofillEligible(true, false, listOf("username", "password")))
        assertFalse(CredentialPolicy.autofillEligible(false, true, listOf("username", "password")))
        assertTrue(CredentialPolicy.autofillEligible(false, true, listOf("username", "totp")))
        assertFalse(CredentialPolicy.autofillEligible(true, false, listOf("username", "totp")))
        assertTrue(CredentialPolicy.autofillEligible(true, true, listOf("username", "password", "totp")))
        assertFalse(CredentialPolicy.autofillEligible(true, false, listOf("password", "totp")))
        assertFalse(CredentialPolicy.autofillEligible(false, true, listOf("password", "totp")))
        assertFalse(CredentialPolicy.autofillEligible(true, false, listOf("totp")))
    }
    @Test fun usernameOnlyAutofillDoesNotRequireAPasswordOrTotp() {
        assertTrue(CredentialPolicy.autofillEligible(false, false, listOf("username")))
        assertTrue(CredentialPolicy.autofillEligible(false, true, listOf("username")))
        assertTrue(CredentialPolicy.autofillEligible(true, false, listOf("username")))
        assertFalse(CredentialPolicy.autofillEligible(true, true, emptyList()))
        assertFalse(CredentialPolicy.autofillEligible(true, true, listOf("unknown")))
    }
    @Test fun otpHintsNeverBecomePasswordFields() {
        assertEquals("totp", CredentialPolicy.field(listOf("smsOTPCode"), "", 0))
        assertEquals("totp", CredentialPolicy.field(emptyList(), "one_time_code", 0))
        assertEquals("password", CredentialPolicy.field(listOf("password"), "", 0))
        assertNull(CredentialPolicy.field(emptyList(), "credit_card", 0))
    }
    @Test fun exactIdentityRejectsUnlistedSignatureAndPackage() {
        val allowed = mapOf("com.browser" to setOf("AA:BB"))
        assertTrue(CredentialPolicy.trustedBrowser("com.browser", setOf("AA:BB"), allowed))
        assertFalse(CredentialPolicy.trustedBrowser("com.browser", setOf("AA:BB", "CC:DD"), allowed))
        assertFalse(CredentialPolicy.trustedBrowser("com.browser.evil", setOf("AA:BB"), allowed))
        assertFalse(CredentialPolicy.trustedBrowser("com.browser", setOf("CC:DD"), allowed))
    }
    @Test fun spoofedWebsiteNeverFallsBackToManualAppScope() {
        assertEquals(Destination.WEBSITE, CredentialPolicy.destination("example.com", true, false))
        assertEquals(Destination.WEBSITE, CredentialPolicy.destination("example.com", false, true))
        assertEquals(Destination.REJECTED, CredentialPolicy.destination("example.com", false, false))
        assertEquals(Destination.MANUAL_APP, CredentialPolicy.destination(null, false, false))
    }
    @Test fun rpCannotEscapeOriginWithSuffixOrUserInfo() {
        assertTrue(CredentialPolicy.rpMatches("https://login.example.com", "example.com"))
        assertFalse(CredentialPolicy.rpMatches("https://evil-example.com", "example.com"))
        assertFalse(CredentialPolicy.rpMatches("https://example.com@evil.com", "example.com"))
        assertFalse(CredentialPolicy.rpMatches("http://example.com", "example.com"))
        assertFalse(CredentialPolicy.rpMatches("https://example.com/path", "example.com"))
        assertFalse(CredentialPolicy.rpMatches("https://example.com", "com"))
    }
    @Test fun selectionExpiresAndIsConsumedEvenOnScopeMismatch() {
        var now = 100L
        val selections = OneTimeSelections<String> { now }
        val first = selections.add("scope-a", "entry")
        assertEquals("entry", selections.take(first, "scope-a"))
        assertNull(selections.take(first, "scope-a"))
        val mismatch = selections.add("scope-a", "entry")
        assertNull(selections.take(mismatch, "scope-b"))
        assertNull(selections.take(mismatch, "scope-a"))
        val stale = selections.add("scope-a", "entry")
        now += 90_001
        assertNull(selections.take(stale, "scope-a"))
    }
    @Test fun cancellationAndProcessRestartDropSelections() {
        val selections = OneTimeSelections<String> { 0 }
        val token = selections.add("scope", "entry")
        selections.cancel(token)
        assertNull(selections.take(token, "scope"))
        val other = selections.add("scope", "entry")
        assertNull(OneTimeSelections<String> { 0 }.take(other, "scope"))
    }
}
