package uk.thomast.myle.mobile

import androidx.credentials.exceptions.CreateCredentialException
import androidx.credentials.exceptions.CreateCredentialUnknownException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.GetCredentialUnknownException
import androidx.credentials.exceptions.domerrors.*
import androidx.credentials.exceptions.publickeycredential.CreatePublicKeyCredentialDomException
import androidx.credentials.exceptions.publickeycredential.GetPublicKeyCredentialDomException

internal object ProviderErrors {
    private fun dom(code: String): DomError? = when (code) {
        "excluded" -> InvalidStateError()
        "notSupported" -> NotSupportedError()
        "badRequest" -> DataError()
        "rpMismatch" -> SecurityError()
        "noCredential" -> NotAllowedError()
        else -> null
    }
    fun create(code: String): CreateCredentialException = dom(code)?.let { CreatePublicKeyCredentialDomException(it) }
        ?: CreateCredentialUnknownException("MYLE could not complete credential creation")
    fun get(code: String): GetCredentialException = dom(code)?.let { GetPublicKeyCredentialDomException(it) }
        ?: GetCredentialUnknownException("MYLE could not complete credential retrieval")
}
