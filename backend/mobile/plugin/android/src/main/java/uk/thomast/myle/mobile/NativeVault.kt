package uk.thomast.myle.mobile

import android.content.Context
import org.json.JSONObject

/** Private in-process JNI boundary; never exposed as an Android IPC or webview command. */
internal object NativeVault {
    init { System.loadLibrary("myle_passwords_lib") }
    @JvmStatic external fun call(dataDir: String, action: String, payload: String): String

    fun invoke(context: Context, action: String, payload: JSONObject = JSONObject()): Any? {
        val response = JSONObject(call(context.applicationInfo.dataDir, action, payload.toString()))
        if (!response.optBoolean("ok")) throw NativeVaultFailure(response.optString("error", "Vault operation failed"))
        return response.opt("result").takeUnless { it == JSONObject.NULL }
    }
    fun cancel(context: Context, session: String?) {
        if (session != null) runCatching { invoke(context, "cancel", JSONObject().put("session", session)) }
    }
}

internal class NativeVaultFailure(val code: String) : Exception(code)
