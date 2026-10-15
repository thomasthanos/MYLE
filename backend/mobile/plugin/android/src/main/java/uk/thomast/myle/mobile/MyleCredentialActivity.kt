package uk.thomast.myle.mobile

import android.content.Intent
import android.content.res.ColorStateList
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.text.InputType
import android.util.Base64
import android.view.WindowManager
import android.view.autofill.AutofillManager
import android.view.autofill.AutofillValue
import android.widget.*
import androidx.biometric.BiometricPrompt
import androidx.credentials.*
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.provider.*
import androidx.fragment.app.FragmentActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.Executors

/** Only explicit provider PendingIntents/internal setup can reach this non-exported activity. */
class MyleCredentialActivity : FragmentActivity() {
    companion object { const val ENROLL = "uk.thomast.myle.mobile.ENROLL_BIOMETRY" }
    private val executor = Executors.newSingleThreadExecutor()
    @Volatile private var ended = false
    @Volatile private var session: String? = null
    private var prompt: BiometricPrompt? = null
    private lateinit var panel: LinearLayout
    private lateinit var message: TextView
    private var ticket: ProviderTicket? = null
    private fun dp(value: Int): Int = (value * resources.displayMetrics.density + 0.5f).toInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        setResult(RESULT_CANCELED)
        if (savedInstanceState != null) { finish(); return }
        panel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(24), dp(24), dp(24))
            setBackgroundColor(Color.rgb(10, 12, 18))
            if (Build.VERSION.SDK_INT >= 26) importantForAutofill = android.view.View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS
        }
        message = TextView(this).apply { setTextColor(Color.WHITE); textSize = 18f; setPadding(0, 0, 0, dp(12)) }
        panel.addView(message)
        ViewCompat.setOnApplyWindowInsetsListener(panel) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
            val margin = dp(24)
            view.setPadding(margin + bars.left, margin + bars.top, margin + bars.right, margin + maxOf(bars.bottom, keyboard.bottom))
            insets
        }
        setContentView(ScrollView(this).apply {
            setBackgroundColor(Color.rgb(10, 12, 18))
            isFillViewport = true
            addView(panel)
        })
        if (intent.action == ENROLL) { enrollment(); return }
        ticket = ProviderTickets.take(intent) { if (!ended) cancelResult() }
        when (val operation = ticket) {
            is ProviderTicket.Query -> query(operation)
            is ProviderTicket.Create -> create(operation)
            is ProviderTicket.Get -> get(operation)
            is ProviderTicket.Autofill -> autofill(operation)
            null -> { message.text = "This request expired. Try again from the requesting app."; button("Close") { finish() } }
        }
    }
    private fun clear(title: String) {
        panel.removeAllViews(); panel.addView(message); message.text = title
    }
    private fun button(label: String, action: () -> Unit) {
        panel.addView(Button(this).apply {
            text = label; isAllCaps = false; filterTouchesWhenObscured = true
            setTextColor(Color.WHITE)
            backgroundTintList = ColorStateList.valueOf(Color.rgb(27, 37, 58))
            minHeight = dp(48)
            setPadding(dp(16), dp(12), dp(16), dp(12))
            layoutParams = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(12) }
            setOnClickListener { if (!ended) action() }
        })
    }
    private fun destination(scope: CredentialScope): String =
        if (scope.manualApp) "${scope.packageName}\nApp certificate: ${scope.certificate}\nChoose a login explicitly for this app."
        else "${scope.site}\nRequested by ${scope.packageName}"

    private fun <T> work(action: () -> T, success: (T) -> Unit, failure: (Exception) -> Unit = { failResult(it) }) {
        if (ended) return
        executor.execute {
            try {
                if (ended) return@execute
                val result = action()
                runOnUiThread { if (!ended) success(result) }
            } catch (error: Exception) { runOnUiThread { if (!ended) failure(error) } }
        }
    }
    private fun failResult(error: Exception) {
        cancelSession()
        if (Build.VERSION.SDK_INT >= 34 && (ticket is ProviderTicket.Create || ticket is ProviderTicket.Get || ticket is ProviderTicket.Query)) {
            val code = if (error is NativeVaultFailure) error.code else if (error is IllegalArgumentException) "rpMismatch" else "unknown"
            val result = Intent()
            if (ticket is ProviderTicket.Create) PendingIntentHandler.setCreateCredentialException(result, ProviderErrors.create(code))
            else PendingIntentHandler.setGetCredentialException(result, ProviderErrors.get(code))
            setResult(RESULT_OK, result); finish()
        } else showError()
    }
    private fun showError(text: String = "MYLE could not complete this request. Return to the requesting app and try again.", retry: (() -> Unit)? = null) {
        cancelSession()
        clear(text)
        if (retry != null) button("Retry request", retry)
        button("Return to app") { cancelResult() }
    }
    private fun cancelSession() {
        val token = session; session = null
        NativeVault.cancel(this, token)
    }
    private fun verify(scope: CredentialScope, action: (String) -> Unit, masterOnly: Boolean = false) {
        cancelSession()
        clear("Verify to continue\n${destination(scope)}")
        val password = EditText(this).apply {
            hint = "Master password"
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            isSaveEnabled = false
            if (Build.VERSION.SDK_INT >= 26) importantForAutofill = android.view.View.IMPORTANT_FOR_AUTOFILL_NO
            filterTouchesWhenObscured = true
            setTextColor(Color.WHITE); setHintTextColor(Color.LTGRAY)
        }
        panel.addView(password)
        button("Verify master password") {
            val input = password.text.toString()
            password.text.clear()
            if (input.isEmpty()) return@button
            clear("Verifying…\n${destination(scope)}")
            executor.execute {
                try {
                    if (ended) return@execute
                    val reply = NativeVault.invoke(this, "unlock", JSONObject().put("password", input).put("scope", scope.json())) as JSONObject
                    val token = reply.getString("session")
                    session = token
                    runOnUiThread {
                        if (ended) NativeVault.cancel(this, token)
                        else action(token)
                    }
                } catch (_: Exception) {
                    runOnUiThread {
                        if (!ended) { verify(scope, action, masterOnly); message.text = "Master password was not verified. Try again.\n${destination(scope)}" }
                    }
                }
            }
        }
        val biometric = ProviderBiometry(this)
        if (!masterOnly && biometric.enabled()) button("Verify fingerprint") {
            prompt = biometric.unlock(this, scope, { token ->
                if (ended) NativeVault.cancel(this, token)
                else { session = token; action(token) }
            }, { if (!ended) verify(scope, action, masterOnly = true) })
        }
        button("Cancel") { cancelResult() }
    }
    private fun payload(token: String, scope: CredentialScope): JSONObject = JSONObject().put("session", token).put("scope", scope.json())
    private fun encode(value: ByteArray): String = Base64.encodeToString(value, Base64.NO_WRAP or Base64.NO_PADDING or Base64.URL_SAFE)
    private fun validateHash(scope: CredentialScope, hash: ByteArray?): String? {
        if (scope.origin.startsWith("https://")) require(hash?.size == 32) { "Browser clientDataHash required" }
        else require(hash == null) { "Native app cannot supply clientDataHash" }
        return hash?.let { encode(it) }
    }
    private fun requestRp(json: String, creation: Boolean): String {
        val request = JSONObject(json)
        return if (creation) request.getJSONObject("rp").getString("id") else request.getString("rpId")
    }
    private fun passkeyPayload(token: String, scope: CredentialScope, json: String, hash: String?): JSONObject {
        val value = payload(token, scope).put("request", JSONObject(json))
        if (hash != null) value.put("clientDataHash", hash)
        return value
    }

    private fun query(operation: ProviderTicket.Query) {
        if (Build.VERSION.SDK_INT < 34) { cancelResult(); return }
        val original = operation.request
        val caller = original.callingAppInfo ?: run { showError(); return }
        val supplied = PendingIntentHandler.retrieveBeginGetCredentialRequest(intent)
        if (supplied != null && (supplied.callingAppInfo != original.callingAppInfo ||
            supplied.beginGetCredentialOptions.map { it.id } != original.beginGetCredentialOptions.map { it.id })) { showError(); return }
        val supported = original.beginGetCredentialOptions.filter { it is BeginGetPasswordOption || it is BeginGetPublicKeyCredentialOption }
        clear("Choose a credential type\nRequested by ${caller.packageName}")
        supported.forEach { option ->
            val json = (option as? BeginGetPublicKeyCredentialOption)?.requestJson
            val title = if (json == null) "Passwords" else "Passkeys for ${runCatching { requestRp(json, false) }.getOrDefault("requested website")}" 
            button(title) {
                work({ CallerVerifier.credential(this, caller, json?.let { requestRp(it, false) }) }, { scope ->
                    val hash = runCatching { validateHash(scope, (option as? BeginGetPublicKeyCredentialOption)?.clientDataHash) }.getOrElse { showError(); return@work }
                    verify(scope, { token ->
                        work({
                            val args = payload(token, scope).put("kind", if (json == null) "password" else "passkey")
                            if (json != null) args.put("request", JSONObject(json))
                            NativeVault.invoke(this, "list", args) as JSONArray
                        }, { entries ->
                            val result = mutableListOf<CredentialEntry>()
                            for (i in 0 until entries.length()) {
                                if (result.size >= 50) break
                                val entry = entries.getJSONObject(i)
                                val storedUsername = entry.optString("username")
                                if (option is BeginGetPasswordOption && !CredentialPolicy.passwordCredentialEligible(entry.optBoolean("hasPassword"), storedUsername)) continue
                                val username = if (option is BeginGetPasswordOption) storedUsername else storedUsername.ifBlank { entry.optString("title", "Login") }
                                if (option is BeginGetPasswordOption && option.allowedUserIds.isNotEmpty() && username !in option.allowedUserIds) continue
                                val credentialId = entry.optString("credentialId").takeIf { it.isNotEmpty() }
                                val selected = ProviderTicket.Get(scope, option.type, json, hash, entry.getString("id"), credentialId)
                                val pending = ProviderTickets.pending(this, selected, true)
                                val credential = when (option) {
                                    is BeginGetPublicKeyCredentialOption -> PublicKeyCredentialEntry(this, username, pending, option, displayName = entry.optString("title"), isAutoSelectAllowed = false)
                                    is BeginGetPasswordOption -> PasswordCredentialEntry(this, username, pending, option, displayName = entry.optString("title"), isAutoSelectAllowed = false)
                                    else -> continue
                                }
                                result.add(credential)
                            }
                            cancelSession()
                            val response = Intent()
                            PendingIntentHandler.setBeginGetCredentialResponse(response, BeginGetCredentialResponse(credentialEntries = result))
                            setResult(RESULT_OK, response); finish()
                        })
                    })
                })
            }
        }
        button("Cancel") { cancelResult() }
    }

    private fun create(operation: ProviderTicket.Create) {
        if (Build.VERSION.SDK_INT < 34) { cancelResult(); return }
        val request = PendingIntentHandler.retrieveProviderCreateCredentialRequest(intent) ?: run { showError(); return }
        val caller = request.callingAppInfo
        val options = request.callingRequest
        if (caller != operation.caller || options.type != operation.type) { showError(); return }
        val json = (options as? CreatePublicKeyCredentialRequest)?.requestJson
        if (json != operation.requestJson) { showError(); return }
        work({ CallerVerifier.credential(this, caller, json?.let { requestRp(it, true) }) }, { scope ->
            val hash = if (json != null) runCatching { validateHash(scope, options.clientDataHash) }.getOrElse { showError(); return@work } else null
            clear("${if (json == null) "Save password" else "Create passkey"}\n${destination(scope)}")
            if (options is CreatePasswordRequest) panel.addView(TextView(this).apply { text = "Username: ${options.id}"; setTextColor(Color.WHITE) })
            button("Confirm and verify") {
                verify(scope, { token ->
                    work({
                        if (json != null) NativeVault.invoke(this, "create", passkeyPayload(token, scope, json, hash)) as JSONObject
                        else {
                            require(options is CreatePasswordRequest)
                            NativeVault.invoke(this, "save", payload(token, scope).put("username", options.id).put("password", options.password))
                            JSONObject()
                        }
                    }, { reply ->
                        session = null
                        val result = Intent()
                        val response = if (json != null) CreatePublicKeyCredentialResponse(reply.toString()) else CreatePasswordResponse()
                        PendingIntentHandler.setCreateCredentialResponse(result, response)
                        setResult(RESULT_OK, result); finish()
                    })
                })
            }
            button("Cancel") { cancelResult() }
        })
    }

    private fun get(operation: ProviderTicket.Get) {
        if (Build.VERSION.SDK_INT < 34) { cancelResult(); return }
        val request = PendingIntentHandler.retrieveProviderGetCredentialRequest(intent) ?: run { showError(); return }
        val options = request.credentialOptions.singleOrNull() ?: run { showError(); return }
        if (options.type != operation.type) { showError(); return }
        val json = (options as? GetPublicKeyCredentialOption)?.requestJson
        if (json != operation.requestJson) { showError(); return }
        work({ CallerVerifier.credential(this, request.callingAppInfo, json?.let { requestRp(it, false) }) }, { scope ->
            if (scope != operation.scope) { showError(); return@work }
            val hash = if (json != null) runCatching { validateHash(scope, options.clientDataHash) }.getOrElse { showError(); return@work } else null
            if (hash != operation.hash) { showError(); return@work }
            verify(scope, { token ->
                work({
                    if (json != null) {
                        require(operation.credentialId != null)
                        val args = passkeyPayload(token, scope, json, hash).put("credentialId", operation.credentialId)
                        PublicKeyCredential((NativeVault.invoke(this, "get", args) as JSONObject).toString())
                    } else {
                        require(options is GetPasswordOption)
                        val allowed = options.allowedUserIds
                        if (allowed.isNotEmpty()) {
                            val entries = NativeVault.invoke(this, "list", payload(token, scope).put("kind", "password")) as JSONArray
                            require((0 until entries.length()).any { entries.getJSONObject(it).getString("id") == operation.id && entries.getJSONObject(it).optString("username") in allowed })
                        }
                        val data = NativeVault.invoke(this, "fill", payload(token, scope).put("id", operation.id).put("fields", JSONArray(listOf("username", "password")))) as JSONObject
                        PasswordCredential(data.getString("username"), data.getString("password"))
                    }
                }, { credential ->
                    session = null
                    val response = Intent()
                    PendingIntentHandler.setGetCredentialResponse(response, GetCredentialResponse(credential))
                    setResult(RESULT_OK, response); finish()
                })
            })
        })
    }

    private fun autofill(operation: ProviderTicket.Autofill) {
        if (Build.VERSION.SDK_INT < 26) { cancelResult(); return }
        if (CredentialPolicy.autofillRequestPackage(operation.form.packageName, packageName) == null) { cancelResult(); return }
        work({ operation.form.resolveScope(this) }, { scope ->
            if (operation.saving) {
                clear("Save password in MYLE\n${destination(scope)}")
                button("Confirm save and verify") {
                    verify(scope, { token ->
                        work({
                            val username = operation.form.fields.firstOrNull { it.kind == "username" }?.node?.autofillValue?.textValue?.toString().orEmpty()
                            val password = operation.form.fields.firstOrNull { it.kind == "password" }?.node?.autofillValue?.textValue?.toString()
                            require(!password.isNullOrEmpty())
                            NativeVault.invoke(this, "save", payload(token, scope).put("username", username).put("password", password))
                        }, { session = null; clear("Password saved in MYLE."); button("Close") { finish() } })
                    })
                }
                button("Cancel") { cancelResult() }
            } else verify(scope, { token ->
                work({ NativeVault.invoke(this, "list", payload(token, scope).put("kind", "password")) as JSONArray }, { entries ->
                    clear("Choose a login\n${destination(scope)}")
                    for (i in 0 until entries.length()) {
                        val entry = entries.getJSONObject(i)
                        if (!CredentialPolicy.autofillEligible(entry.optBoolean("hasPassword"), entry.optBoolean("hasTotp"), operation.form.requested())) continue
                        button("${entry.optString("title", "Login")}\n${entry.optString("username")}") {
                            clear("Filling selected login…")
                            work({ NativeVault.invoke(this, "fill", payload(token, scope).put("id", entry.getString("id")).put("fields", JSONArray(operation.form.requested()))) as JSONObject }, filling@ { values ->
                                session = null
                                val presentation = RemoteViews(packageName, android.R.layout.simple_list_item_1).apply { setTextViewText(android.R.id.text1, "MYLE • selected login") }
                                val dataset = android.service.autofill.Dataset.Builder(presentation)
                                var count = 0
                                operation.form.fields.forEach { field ->
                                    if (values.has(field.kind) && !values.isNull(field.kind)) {
                                        dataset.setValue(field.id, AutofillValue.forText(values.getString(field.kind)))
                                        count++
                                    }
                                }
                                if (count == 0) { showError(); return@filling }
                                val response = android.service.autofill.FillResponse.Builder().addDataset(dataset.build()).build()
                                setResult(RESULT_OK, Intent().putExtra(AutofillManager.EXTRA_AUTHENTICATION_RESULT, response))
                                finish()
                            })
                        }
                    }
                    button("Cancel") { cancelResult() }
                })
            })
        }, { showError("MYLE could not verify this website or app. Retry the identity check or return to the requesting app.") { autofill(operation) } })
    }
    private fun enrollment() {
        work({ CallerVerifier.ownApp(this) }, { scope ->
            verify(scope, { token ->
                work({ NativeVault.invoke(this, "enrollKey", payload(token, scope)) as JSONObject }, { reply ->
                    session = null
                    val key = reply.getString("key").toByteArray(Charsets.UTF_8)
                    prompt = ProviderBiometry(this).enroll(this, key, reply.getString("vaultId"), {
                        clear("Fingerprint unlock enabled. Each credential operation requires verification.")
                        button("Close") { finish() }
                    }, { if (!ended) { clear("Fingerprint setup was not completed. Master password remains available."); button("Close") { finish() } } })
                })
            }, masterOnly = true)
        })
    }
    private fun cancelResult() {
        cancelSession()
        if (Build.VERSION.SDK_INT >= 34) {
            val result = Intent()
            when (ticket) {
                is ProviderTicket.Create -> PendingIntentHandler.setCreateCredentialException(result, CreateCredentialCancellationException())
                is ProviderTicket.Query, is ProviderTicket.Get -> PendingIntentHandler.setGetCredentialException(result, GetCredentialCancellationException())
                else -> Unit
            }
            setResult(RESULT_OK, result)
        }
        finish()
    }
    @Suppress("DEPRECATION") override fun onBackPressed() { cancelResult() }
    override fun onDestroy() {
        ended = true
        ProviderTickets.detach(intent)
        prompt?.cancelAuthentication()
        cancelSession()
        executor.shutdownNow()
        super.onDestroy()
    }
}
