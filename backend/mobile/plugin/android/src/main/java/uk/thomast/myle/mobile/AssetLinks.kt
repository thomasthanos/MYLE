package uk.thomast.myle.mobile

import org.json.JSONArray

internal object AssetLinks {
    fun matches(statements: JSONArray, pkg: String, certificate: String): Boolean {
        for (i in 0 until statements.length()) {
            val statement = statements.optJSONObject(i) ?: continue
            val relations = statement.optJSONArray("relation") ?: continue
            if ((0 until relations.length()).none { relations.optString(it) == "delegate_permission/common.get_login_creds" }) continue
            val target = statement.optJSONObject("target") ?: continue
            if (target.optString("namespace") != "android_app" || target.optString("package_name") != pkg) continue
            val fingerprints = target.optJSONArray("sha256_cert_fingerprints") ?: continue
            if ((0 until fingerprints.length()).any { fingerprints.optString(it).uppercase() == certificate }) return true
        }
        return false
    }
}

