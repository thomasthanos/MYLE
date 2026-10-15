package uk.thomast.myle.mobile

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import android.util.Base64
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import org.json.JSONObject
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** A vault key exists only inside the authentication callback; the stored copy is AES-GCM ciphertext. */
internal class ProviderBiometry(private val context: Context) {
    private val alias = "myle-provider-vault-key-v1"
    private val file get() = AtomicFile(File(context.noBackupFilesDir, "provider-biometry.json"))
    private fun keystore() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    fun available(): Boolean = BiometricManager.from(context).canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS
    fun enabled(): Boolean = available() && file.baseFile.exists() && runCatching { keystore().containsAlias(alias) }.getOrDefault(false)
    fun disable() {
        file.delete()
        runCatching { keystore().deleteEntry(alias) }
    }
    private fun cipher(mode: Int, record: JSONObject? = null): Cipher {
        val store = keystore()
        if (mode == Cipher.ENCRYPT_MODE) {
            val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            val builder = KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setUserAuthenticationRequired(true).setInvalidatedByBiometricEnrollment(true)
            if (Build.VERSION.SDK_INT >= 30) builder.setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
            else builder.setUserAuthenticationValidityDurationSeconds(-1)
            generator.init(builder.build())
            generator.generateKey()
        }
        val result = Cipher.getInstance("AES/GCM/NoPadding")
        val key = store.getKey(alias, null) as SecretKey
        if (record == null) result.init(mode, key)
        else {
            result.init(mode, key, GCMParameterSpec(128, Base64.decode(record.getString("iv"), Base64.NO_WRAP or Base64.URL_SAFE)))
            result.updateAAD(record.getString("vaultId").toByteArray(Charsets.UTF_8))
        }
        return result
    }
    fun unlock(activity: FragmentActivity, scope: CredentialScope, success: (String) -> Unit, fallback: () -> Unit): BiometricPrompt? {
        if (!enabled()) { fallback(); return null }
        return try {
            val record = JSONObject(file.openRead().bufferedReader().use { it.readText() })
            val operation = cipher(Cipher.DECRYPT_MODE, record)
            prompt(activity, operation, "Unlock MYLE", { authenticated ->
                var key: ByteArray? = null
                try {
                    key = authenticated.doFinal(Base64.decode(record.getString("ciphertext"), Base64.NO_WRAP or Base64.URL_SAFE))
                    val reply = NativeVault.invoke(context, "unlockKey", BiometricKeyPayload.unlock(key, record.getString("vaultId"), scope.json())) as JSONObject
                    success(reply.getString("session"))
                } catch (_: Exception) {
                    // Includes enrollment invalidation, changed/replaced vault and authentication-tag failures.
                    disable(); fallback()
                } finally { key?.fill(0) }
            }, fallback)
        } catch (_: Exception) { disable(); fallback(); null }
    }
    fun enroll(activity: FragmentActivity, key: ByteArray, vaultId: String, success: () -> Unit, failure: () -> Unit): BiometricPrompt? {
        return try {
            require(available())
            disable()
            val operation = cipher(Cipher.ENCRYPT_MODE)
            operation.updateAAD(vaultId.toByteArray(Charsets.UTF_8))
            prompt(activity, operation, "Enable fingerprint for MYLE", { authenticated ->
                try {
                    val ciphertext = authenticated.doFinal(key)
                    val record = JSONObject().put("vaultId", vaultId).put("iv", encode(authenticated.iv)).put("ciphertext", encode(ciphertext))
                    val stream = file.startWrite()
                    try { stream.write(record.toString().toByteArray(Charsets.UTF_8)); file.finishWrite(stream) }
                    catch (e: Exception) { file.failWrite(stream); throw e }
                    success()
                } catch (_: Exception) { disable(); failure() }
                finally { key.fill(0) }
            }, { key.fill(0); disable(); failure() })
        } catch (_: Exception) { key.fill(0); disable(); failure(); null }
    }
    private fun encode(value: ByteArray): String = Base64.encodeToString(value, Base64.NO_WRAP or Base64.NO_PADDING or Base64.URL_SAFE)
    private fun prompt(activity: FragmentActivity, cipher: Cipher, title: String, success: (Cipher) -> Unit, fallback: () -> Unit): BiometricPrompt {
        val prompt = BiometricPrompt(activity, ContextCompat.getMainExecutor(activity), object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                val authenticated = result.cryptoObject?.cipher
                if (authenticated == null) fallback() else success(authenticated)
            }
            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) { fallback() }
        })
        val info = BiometricPrompt.PromptInfo.Builder().setTitle(title)
            .setSubtitle("Verify this operation with a strong biometric")
            .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
            .setNegativeButtonText("Use master password").build()
        prompt.authenticate(info, BiometricPrompt.CryptoObject(cipher))
        return prompt
    }
}

