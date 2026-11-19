package com.faforever.rustclient

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

// The client's secrets on Android, the remembered sign-in among them: each
// encrypted with an AES key that Android's keystore holds and never hands
// out, and kept in this app's private preferences. The Rust side reaches
// them as keyring's credential store (src/keystore.rs); without it keyring
// keeps them in memory only, and the sign-in was gone at every start.
private const val KEY_ALIAS = "faf-client-secrets"
private const val PREFERENCES = "secure-store"
private const val IV_BYTES = 12
private const val TAG_BITS = 128

@InvokeArg
class SecretArgs {
  var key: String = ""
  var value: String? = null
}

@TauriPlugin
class SecureStorePlugin(private val activity: Activity) : Plugin(activity) {
  private val preferences by lazy { activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE) }

  // The secret under `key`, or no `value` when there is none. One that no
  // longer opens (its key is gone: a restore onto another phone, a wiped
  // keystore) is dropped, as if it had never been kept.
  @Command
  fun get(invoke: Invoke) {
    val args = invoke.parseArgs(SecretArgs::class.java)
    val answer = JSObject()
    val sealed = preferences.getString(args.key, null)
    if (sealed != null) {
      try {
        answer.put("value", open(sealed))
      } catch (lost: GeneralSecurityException) {
        preferences.edit().remove(args.key).commit()
      } catch (broken: IllegalArgumentException) {
        preferences.edit().remove(args.key).commit()
      }
    }
    invoke.resolve(answer)
  }

  @Command
  fun set(invoke: Invoke) {
    val args = invoke.parseArgs(SecretArgs::class.java)
    try {
      if (!preferences.edit().putString(args.key, seal(args.value ?: "")).commit()) {
        invoke.reject("the secret could not be written")
        return
      }
      invoke.resolve()
    } catch (refused: GeneralSecurityException) {
      invoke.reject("the keystore refused the secret: ${refused.message}")
    }
  }

  // Whether there was a secret under `key` to delete.
  @Command
  fun delete(invoke: Invoke) {
    val args = invoke.parseArgs(SecretArgs::class.java)
    val answer = JSObject()
    answer.put("deleted", preferences.contains(args.key))
    preferences.edit().remove(args.key).commit()
    invoke.resolve(answer)
  }

  private fun key(): SecretKey {
    val keystore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (keystore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(
      KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build(),
    )
    return generator.generateKey()
  }

  // The IV, then the ciphertext with its tag, in Base64.
  private fun seal(plain: String): String {
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.ENCRYPT_MODE, key())
    val sealed = cipher.iv + cipher.doFinal(plain.toByteArray(Charsets.UTF_8))
    return Base64.encodeToString(sealed, Base64.NO_WRAP)
  }

  private fun open(stored: String): String {
    val sealed = Base64.decode(stored, Base64.NO_WRAP)
    if (sealed.size <= IV_BYTES) throw IllegalArgumentException("too short to be a sealed secret")
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, sealed, 0, IV_BYTES))
    return String(cipher.doFinal(sealed, IV_BYTES, sealed.size - IV_BYTES), Charsets.UTF_8)
  }
}
