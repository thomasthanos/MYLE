package uk.thomast.myle.mobile

import android.app.assist.AssistStructure
import android.content.Context
import android.os.Build
import android.os.CancellationSignal
import android.service.autofill.*
import android.view.autofill.AutofillId
import android.widget.RemoteViews
import androidx.annotation.RequiresApi

internal data class AutofillField(val id: AutofillId, val kind: String, val node: AssistStructure.ViewNode?)
internal data class AutofillForm(val packageName: String, val domain: String?, val scheme: String?, val fields: List<AutofillField>) {
    fun requested(): List<String> = fields.map { it.kind }.distinct()
    fun resolveScope(context: Context): CredentialScope = CallerVerifier.autofill(context, packageName, domain, scheme)
}

@RequiresApi(26)
internal object AutofillParser {
    fun parse(structure: AssistStructure): AutofillForm? {
        val pkg = structure.activityComponent?.packageName ?: return null
        val fields = mutableListOf<AutofillField>()
        val domains = mutableSetOf<Pair<String?, String?>>()
        val claims = mutableSetOf<String>()
        fun visit(node: AssistStructure.ViewNode, domain: String?, scheme: String?) {
            node.webDomain?.takeIf { it.isNotBlank() }?.let { claims.add(it) }
            val currentDomain = node.webDomain?.takeIf { it.isNotBlank() } ?: domain
            val currentScheme = if (Build.VERSION.SDK_INT >= 28) node.webScheme ?: scheme else scheme
            val html = node.htmlInfo?.attributes?.firstOrNull { it.first == "autocomplete" }?.second
            val hints = node.autofillHints?.toList().orEmpty() + listOfNotNull(html)
            val kind = CredentialPolicy.field(hints, node.idEntry ?: "", node.inputType)
            val id = node.autofillId
            if (kind != null && id != null && node.autofillType == android.view.View.AUTOFILL_TYPE_TEXT) {
                fields.add(AutofillField(id, kind, node))
                domains.add(currentDomain to currentScheme)
            }
            for (i in 0 until node.childCount) visit(node.getChildAt(i), currentDomain, currentScheme)
        }
        for (i in 0 until structure.windowNodeCount) visit(structure.getWindowNodeAt(i).rootViewNode, null, null)
        // A frame with a conflicting website identity must never receive another frame's login.
        if (fields.isEmpty() || domains.size != 1 || !CredentialPolicy.consistentFormDomains(domains.map { it.first }.toSet(), claims)) return null
        val (domain, scheme) = domains.single()
        return AutofillForm(pkg, domain, scheme, fields.distinctBy { it.id })
    }
}

@RequiresApi(26)
class MyleAutofillService : AutofillService() {
    override fun onFillRequest(request: FillRequest, cancellationSignal: CancellationSignal, callback: FillCallback) {
        if (cancellationSignal.isCanceled) return
        val form = runCatching { AutofillParser.parse(request.fillContexts.last().structure) }.getOrNull()
        if (form == null) { callback.onSuccess(null); return }
        val pending = ProviderTickets.pending(this, ProviderTicket.Autofill(form.copy(fields = form.fields.map { it.copy(node = null) })), false)
        cancellationSignal.setOnCancelListener { ProviderTickets.cancel(pending) }
        val presentation = RemoteViews(packageName, android.R.layout.simple_list_item_1).apply {
            setTextViewText(android.R.id.text1, "Unlock MYLE • passwords and 2FA")
        }
        val ids = form.fields.map { it.id }.toTypedArray()
        val response = FillResponse.Builder().setAuthentication(ids, pending.intentSender, presentation)
        val passwords = form.fields.filter { it.kind == "password" }.map { it.id }
        val usernames = form.fields.filter { it.kind == "username" }.map { it.id }
        if (passwords.isNotEmpty()) response.setSaveInfo(SaveInfo.Builder(SaveInfo.SAVE_DATA_TYPE_PASSWORD, (usernames + passwords).toTypedArray()).build())
        if (!cancellationSignal.isCanceled) callback.onSuccess(response.build())
    }

    override fun onSaveRequest(request: SaveRequest, callback: SaveCallback) {
        val form = runCatching { AutofillParser.parse(request.fillContexts.last().structure) }.getOrNull()
        if (form == null || !form.requested().contains("password")) { callback.onFailure("No password fields to save"); return }
        val pending = ProviderTickets.pending(this, ProviderTicket.Autofill(form, saving = true), false)
        // API 28+ lets Android launch the private verification/confirmation UI after its own save prompt.
        if (Build.VERSION.SDK_INT >= 28) callback.onSuccess(pending.intentSender)
        else {
            // API 26/27 cannot delegate save verification through SaveCallback.
            ProviderTickets.cancel(pending)
            callback.onFailure("Open MYLE to save this password on this Android version")
        }
    }
}
