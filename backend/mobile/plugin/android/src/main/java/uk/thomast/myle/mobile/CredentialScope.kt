package uk.thomast.myle.mobile

import android.content.Context
import android.content.pm.PackageManager
import android.content.pm.Signature
import android.os.Build
import android.util.Base64
import androidx.credentials.provider.CallingAppInfo
import org.json.JSONArray
import org.json.JSONObject
import java.net.URL
import java.security.MessageDigest
import javax.net.ssl.HttpsURLConnection

internal data class CredentialScope(val origin: String, val site: String, val packageName: String, val certificate: String) {
    fun json(): JSONObject = JSONObject().put("origin", origin).put("site", site).put("packageName", packageName).put("certificate", certificate)
    fun binding(): String = json().toString()
    val manualApp: Boolean get() = origin.startsWith("android:apk-key-hash:") && site == packageName
}

internal object CallerVerifier {
    private fun digest(signature: Signature) = MessageDigest.getInstance("SHA-256").digest(signature.toByteArray())
    private fun encoded(bytes: ByteArray) = Base64.encodeToString(bytes, Base64.NO_WRAP or Base64.NO_PADDING or Base64.URL_SAFE)
    private fun fingerprint(bytes: ByteArray) = bytes.joinToString(":") { "%02X".format(it.toInt() and 255) }
    @Suppress("DEPRECATION")
    fun signatures(context: Context, pkg: String): List<Signature> {
        return if (Build.VERSION.SDK_INT >= 28) {
            val info = context.packageManager.getPackageInfo(pkg, PackageManager.GET_SIGNING_CERTIFICATES)
            info.signingInfo?.apkContentsSigners?.toList() ?: emptyList()
        } else context.packageManager.getPackageInfo(pkg, PackageManager.GET_SIGNATURES).signatures?.toList() ?: emptyList()
    }
    private fun allowlistJson(context: Context): String = context.assets.open("privileged-browsers.json").bufferedReader().use { it.readText() }
    fun allowlist(context: Context): Map<String, Set<String>> {
        val apps = JSONObject(allowlistJson(context)).getJSONArray("apps")
        val result = mutableMapOf<String, Set<String>>()
        for (i in 0 until apps.length()) {
            val app = apps.getJSONObject(i)
            if (app.optString("type") != "android") continue
            val info = app.getJSONObject("info")
            val signatures = info.getJSONArray("signatures")
            val allowed = mutableSetOf<String>()
            for (j in 0 until signatures.length()) {
                val signature = signatures.getJSONObject(j)
                // Development certificates must never gain production browser privileges.
                if (signature.optString("build") == "userdebug") continue
                allowed.add(signature.getString("cert_fingerprint_sha256").uppercase())
            }
            result[info.getString("package_name")] = allowed
        }
        return result
    }
    fun trusted(context: Context, pkg: String, signatures: List<Signature>): Boolean =
        CredentialPolicy.trustedBrowser(pkg, signatures.map { fingerprint(digest(it)) }.toSet(), allowlist(context))

    private fun identity(pkg: String, signatures: List<Signature>): CredentialScope {
        // The vault scope has one exact current certificate, so reject multi-signed apps.
        require(signatures.size == 1) { "This app's signing identity is unsupported" }
        val certificate = encoded(digest(signatures.single()))
        return CredentialScope("android:apk-key-hash:$certificate", pkg, pkg, certificate)
    }
    fun ownApp(context: Context): CredentialScope = identity(context.packageName, signatures(context, context.packageName))

    /** Must run off the UI thread: native app website/RP claims require live HTTPS asset links. */
    fun credential(context: Context, caller: CallingAppInfo, rp: String?): CredentialScope {
        val actual = signatures(context, caller.packageName)
        val supplied = caller.signingInfoCompat.apkContentsSigners
        require(actual.map { encoded(digest(it)) }.toSet() == supplied.map { encoded(digest(it)) }.toSet()) { "Caller identity changed" }
        val native = identity(caller.packageName, actual)
        val browserOrigin = caller.getOrigin(allowlistJson(context))
        if (browserOrigin != null) {
            val site = CredentialPolicy.credentialBrowserSite(browserOrigin, rp, trusted(context, caller.packageName, actual))
            return native.copy(origin = browserOrigin, site = site)
        }
        if (rp == null) return native
        val site = CredentialPolicy.host(rp)
        require(assetLinked(site, native.packageName, fingerprint(digest(actual.single())))) { "Website has not authorized this app" }
        return native.copy(site = site)
    }

    fun autofill(context: Context, pkg: String, claimedDomain: String?, scheme: String?): CredentialScope {
        val signers = signatures(context, pkg)
        val native = identity(pkg, signers)
        val domain = claimedDomain?.takeIf { it.isNotEmpty() }?.let { CredentialPolicy.host(it) }
        val browser = trusted(context, pkg, signers)
        val linked = domain != null && !browser && assetLinked(domain, pkg, fingerprint(digest(signers.single())))
        return when (CredentialPolicy.destination(domain, browser, linked)) {
            Destination.MANUAL_APP -> native
            Destination.REJECTED -> error("Unverified website claim")
            Destination.WEBSITE -> {
                require(domain != null)
                if (browser) {
                    require(scheme == "https") { "Only verified HTTPS browser pages can be filled" }
                    native.copy(origin = "https://$domain", site = domain)
                } else native.copy(site = domain)
            }
        }
    }

    private fun assetLinked(site: String, pkg: String, fingerprint: String): Boolean {
        val connection = URL("https://$site/.well-known/assetlinks.json").openConnection() as HttpsURLConnection
        connection.connectTimeout = 5000
        connection.readTimeout = 5000
        connection.instanceFollowRedirects = false
        try {
            if (connection.responseCode != 200) return false
            val bytes = connection.inputStream.use { it.readBytesLimited(262_144) }
            return AssetLinks.matches(JSONArray(String(bytes, Charsets.UTF_8)), pkg, fingerprint)
        } finally { connection.disconnect() }
    }
    private fun java.io.InputStream.readBytesLimited(limit: Int): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        val buffer = ByteArray(4096)
        while (true) {
            val length = read(buffer)
            if (length < 0) return out.toByteArray()
            require(out.size() + length <= limit) { "Asset links response is too large" }
            out.write(buffer, 0, length)
        }
    }
}


