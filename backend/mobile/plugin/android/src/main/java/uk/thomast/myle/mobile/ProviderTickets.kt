package uk.thomast.myle.mobile

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.SystemClock
import android.os.Handler
import android.os.Looper
import androidx.credentials.provider.BeginGetCredentialRequest
import androidx.credentials.provider.CallingAppInfo
import java.util.concurrent.atomic.AtomicInteger

/** The PendingIntent carries only an opaque ID. Scope/selection is never taken from mutable extras. */
internal sealed class ProviderTicket {
    data class Query(val request: BeginGetCredentialRequest) : ProviderTicket()
    data class Create(val type: String, val caller: CallingAppInfo, val requestJson: String?) : ProviderTicket()
    data class Get(val scope: CredentialScope, val type: String, val requestJson: String?, val hash: String?, val id: String, val credentialId: String?) : ProviderTicket()
    data class Autofill(val form: AutofillForm, val saving: Boolean = false) : ProviderTicket()
}
internal object ProviderTickets {
    private val tickets = OneTimeSelections<ProviderTicket> { SystemClock.elapsedRealtime() }
    private val counter = AtomicInteger()
    private val main = Handler(Looper.getMainLooper())
    private data class Handle(val pending: PendingIntent, var active: (() -> Unit)? = null)
    private val handles = LinkedHashMap<String, Handle>()
    private const val ACTION = "uk.thomast.myle.mobile.CREDENTIAL_OPERATION"
    @Synchronized fun pending(context: Context, ticket: ProviderTicket, frameworkExtras: Boolean): PendingIntent {
        val id = tickets.add(ACTION, ticket)
        val intent = Intent(context, MyleCredentialActivity::class.java).setAction(ACTION)
            .setData(Uri.parse("myle-provider://request/$id"))
        val flags = if (frameworkExtras && android.os.Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else PendingIntent.FLAG_IMMUTABLE
        val pending = PendingIntent.getActivity(context, counter.incrementAndGet(), intent, flags)
        while (handles.size >= 128) cancelId(handles.keys.first())
        handles[id] = Handle(pending)
        main.postDelayed({ cancelId(id) }, 90_000)
        return pending
    }
    @Synchronized fun take(intent: Intent, onCancel: () -> Unit): ProviderTicket? {
        if (intent.data?.scheme != "myle-provider" || intent.data?.host != "request") return null
        val id = intent.data?.lastPathSegment ?: return null
        val handle = handles[id] ?: return null
        val ticket = tickets.take(id, intent.action ?: "") ?: return null
        handle.active = onCancel
        return ticket
    }
    @Synchronized private fun cancelId(id: String) {
        tickets.cancel(id)
        val handle = handles.remove(id) ?: return
        handle.pending.cancel()
        handle.active?.let { main.post(it) }
    }
    @Synchronized fun cancel(pending: PendingIntent) {
        val id = handles.entries.firstOrNull { it.value.pending == pending }?.key
        if (id != null) cancelId(id) else pending.cancel()
    }
    @Synchronized fun detach(intent: Intent) {
        val id = intent.data?.lastPathSegment ?: return
        tickets.cancel(id)
        handles.remove(id)?.pending?.cancel()
    }
    @Synchronized fun clear() {
        handles.keys.toList().forEach { cancelId(it) }
        tickets.clear()
    }
}
