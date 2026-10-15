package uk.thomast.myle.mobile

import android.os.CancellationSignal
import android.os.OutcomeReceiver
import androidx.annotation.RequiresApi
import androidx.credentials.CreatePasswordRequest
import androidx.credentials.PublicKeyCredential
import androidx.credentials.PasswordCredential
import androidx.credentials.exceptions.*
import androidx.credentials.provider.*

@RequiresApi(34)
class MyleCredentialProviderService : CredentialProviderService() {
    override fun onBeginGetCredentialRequest(request: BeginGetCredentialRequest, cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<BeginGetCredentialResponse, GetCredentialException>) {
        if (cancellationSignal.isCanceled) { callback.onError(GetCredentialCancellationException()); return }
        if (request.callingAppInfo == null || request.beginGetCredentialOptions.none { it is BeginGetPublicKeyCredentialOption || it is BeginGetPasswordOption }) {
            callback.onResult(BeginGetCredentialResponse()); return
        }
        val pending = ProviderTickets.pending(this, ProviderTicket.Query(request), true)
        cancellationSignal.setOnCancelListener { ProviderTickets.cancel(pending) }
        if (!cancellationSignal.isCanceled) callback.onResult(BeginGetCredentialResponse(authenticationActions = listOf(AuthenticationAction("Unlock MYLE", pending))))
    }

    override fun onBeginCreateCredentialRequest(request: BeginCreateCredentialRequest, cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<BeginCreateCredentialResponse, CreateCredentialException>) {
        if (cancellationSignal.isCanceled) { callback.onError(CreateCredentialCancellationException()); return }
        val caller = request.callingAppInfo ?: run { callback.onError(CreateCredentialUnknownException("Missing caller")); return }
        val json = when (request) {
            is BeginCreatePublicKeyCredentialRequest -> request.requestJson
            is BeginCreatePasswordCredentialRequest -> null
            else -> { callback.onError(CreateCredentialUnsupportedException()); return }
        }
        val pending = ProviderTickets.pending(this, ProviderTicket.Create(request.type, caller, json), true)
        cancellationSignal.setOnCancelListener { ProviderTickets.cancel(pending) }
        if (!cancellationSignal.isCanceled) callback.onResult(BeginCreateCredentialResponse(listOf(CreateEntry("MYLE vault", pending, description = "Confirm and verify to save"))))
    }

    override fun onClearCredentialStateRequest(request: ProviderClearCredentialStateRequest, cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<Void?, ClearCredentialException>) {
        ProviderTickets.clear()
        if (!cancellationSignal.isCanceled) callback.onResult(null)
        else callback.onError(ClearCredentialUnknownException("Request canceled"))
    }
}
