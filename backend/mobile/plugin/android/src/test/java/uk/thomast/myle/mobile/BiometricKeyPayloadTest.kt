package uk.thomast.myle.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class BiometricKeyPayloadTest {
    @Test fun opaqueKeyAndAuthenticatedVaultIdArePassedUnchanged() {
        val payload = BiometricKeyPayload.unlock("opaque+/key==".toByteArray(), "vault-a", JSONObject().put("site", "com.example"))
        assertEquals("opaque+/key==", payload.getString("key"))
        assertEquals("vault-a", payload.getString("vaultId"))
        assertEquals("com.example", payload.getJSONObject("scope").getString("site"))
    }
}
