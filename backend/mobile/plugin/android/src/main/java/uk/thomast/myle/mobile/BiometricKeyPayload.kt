package uk.thomast.myle.mobile

import org.json.JSONObject

/** Preserve the vault's opaque key encoding and bind it to the AES-GCM authenticated vault ID. */
internal object BiometricKeyPayload {
    fun unlock(key: ByteArray, vaultId: String, scope: JSONObject): JSONObject =
        JSONObject().put("key", String(key, Charsets.UTF_8)).put("vaultId", vaultId).put("scope", scope)
}
