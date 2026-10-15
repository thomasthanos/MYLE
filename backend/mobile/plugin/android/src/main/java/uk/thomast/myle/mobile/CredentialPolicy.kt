package uk.thomast.myle.mobile

import java.net.URI
import java.util.Locale
import java.util.UUID
import com.google.common.net.InternetDomainName

internal enum class Destination { WEBSITE, MANUAL_APP, REJECTED }

internal object CredentialPolicy {
    fun passwordCredentialEligible(hasPassword: Boolean, username: String): Boolean =
        hasPassword && username.isNotEmpty()

    fun autofillEligible(hasPassword: Boolean, hasTotp: Boolean, requested: List<String>): Boolean =
        requested.isNotEmpty() && requested.all {
            when (it) {
                "username" -> true
                "password" -> hasPassword
                "totp" -> hasTotp
                else -> false
            }
        }

    fun field(hints: List<String>, id: String, inputType: Int): String? {
        val names = (hints + id).map { it.lowercase(Locale.ROOT).replace("_", "").replace("-", "") }
        if (names.any { it in setOf("smsotpcode", "onetimecode", "otp", "otpcode", "totp", "verificationcode", "2fa") }) return "totp"
        if (names.any { it in setOf("password", "newpassword", "currentpassword") }) return "password"
        if ((inputType and 0xfff) in setOf(0x81, 0x91, 0xe1)) return "password"
        if (names.any { it in setOf("username", "emailaddress", "email", "login", "userid") }) return "username"
        return null
    }
    fun trustedBrowser(pkg: String, certificates: Set<String>, allowlist: Map<String, Set<String>>): Boolean =
        certificates.isNotEmpty() && allowlist[pkg]?.containsAll(certificates) == true

    fun destination(claim: String?, trustedBrowser: Boolean, assetLinked: Boolean): Destination =
        if (claim.isNullOrEmpty()) Destination.MANUAL_APP else if (trustedBrowser || assetLinked) Destination.WEBSITE else Destination.REJECTED

    fun consistentFormDomains(fieldDomains: Set<String?>, allClaims: Set<String>): Boolean =
        fieldDomains.size == 1 && (allClaims.isEmpty() || (fieldDomains.single() != null && allClaims.all { it == fieldDomains.single() }))

    fun host(value: String): String {
        require(value == value.lowercase(Locale.ROOT) && value.length <= 253 && !value.endsWith('.'))
        require(value.matches(Regex("[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?")) && !value.contains(".."))
        require(InternetDomainName.from(value).isUnderPublicSuffix) { "Destination must be a registrable domain" }
        return value
    }
    fun originHost(origin: String): String {
        val uri = URI(origin)
        require(uri.scheme == "https" && uri.userInfo == null && uri.host != null)
        require(uri.rawPath.isNullOrEmpty() && uri.rawQuery == null && uri.rawFragment == null)
        require(uri.port in -1..65535)
        return host(uri.host)
    }
    fun rpMatches(origin: String, rp: String): Boolean = runCatching {
        val destination = originHost(origin)
        val site = host(rp)
        destination == site || destination.endsWith(".$site")
    }.getOrDefault(false)

    /** Only a framework origin from an exactly allowlisted browser may use Related Origin Requests. */
    fun credentialBrowserSite(origin: String, rp: String?, trusted: Boolean): String {
        require(trusted) { "Browser is not trusted" }
        val originHost = originHost(origin)
        // Credential Manager browsers have already checked the RP's Related Origin authorization.
        // Autofill does not use this policy: its destination always remains the actual page host.
        return rp?.let { host(it) } ?: originHost
    }
}

/** Process-local non-secret selection tickets; cancellation, expiry and scope mismatch consume them. */
internal class OneTimeSelections<T>(private val clock: () -> Long) {
    private data class Selection<T>(val scope: String, val value: T, val expires: Long)
    private val entries = LinkedHashMap<String, Selection<T>>()
    @Synchronized fun add(scope: String, value: T): String {
        entries.entries.removeAll { it.value.expires <= clock() }
        while (entries.size >= 128) entries.remove(entries.keys.first())
        val id = UUID.randomUUID().toString()
        entries[id] = Selection(scope, value, clock() + 90_000)
        return id
    }
    @Synchronized fun take(id: String, scope: String): T? {
        val entry = entries.remove(id) ?: return null
        return entry.value.takeIf { entry.scope == scope && clock() < entry.expires }
    }
    @Synchronized fun cancel(id: String) { entries.remove(id) }
    @Synchronized fun clear() { entries.clear() }
}
