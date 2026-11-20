package io.nyx.browser

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Clé d'appareil du coffre, au repos : 32 octets aléatoires enveloppés par
 * une clé AES-256-GCM du Keystore Android (alias [ALIAS], non exportable,
 * sans authentification utilisateur — ouverture silencieuse, comme la clé
 * DPAPI de Windows). Fichier app-privé `filesDir/vault.key.enc` :
 *
 *     "NYXKEYA1" | longueur IV (1 octet) | IV | AES-GCM(clé brute) + tag
 *
 * Écriture atomique ([AtomicFile] : un plantage laisse l'ancien fichier ou
 * le nouveau). La clé Keystore n'est jamais supprimée : un fichier
 * illisible fait retomber sur la saisie du mot de passe principal, rien
 * n'est effacé automatiquement.
 */
object KeyStoreWrap {
    const val ALIAS = "nyx_vault_wrap"
    private const val PROVIDER = "AndroidKeyStore"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val TAG_BITS = 128
    private val MAGIC = "NYXKEYA1".toByteArray(Charsets.US_ASCII)

    private fun secretKey(): SecretKey {
        val ks = KeyStore.getInstance(PROVIDER)
        ks.load(null)
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setUserAuthenticationRequired(false)
                .build(),
        )
        return generator.generateKey()
    }

    /** Enveloppe `plain` (IV aléatoire choisi par le Keystore). */
    @Throws(GeneralSecurityException::class)
    fun wrap(plain: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, secretKey())
        val iv = cipher.iv
        val sealed = cipher.doFinal(plain)
        return MAGIC + byteArrayOf(iv.size.toByte()) + iv + sealed
    }

    /** Désenveloppe ; exception si fichier étranger, tronqué ou clé changée. */
    @Throws(GeneralSecurityException::class)
    fun unwrap(blob: ByteArray): ByteArray {
        if (blob.size < MAGIC.size + 1 || !blob.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) {
            throw GeneralSecurityException("fichier de clé non reconnu")
        }
        val ivLen = blob[MAGIC.size].toInt() and 0xff
        val start = MAGIC.size + 1
        if (ivLen < 12 || blob.size <= start + ivLen) throw GeneralSecurityException("fichier de clé tronqué")
        val iv = blob.copyOfRange(start, start + ivLen)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(TAG_BITS, iv))
        return cipher.doFinal(blob, start + ivLen, blob.size - start - ivLen)
    }

    /** Écrit la clé enveloppée de façon atomique. */
    @Throws(IOException::class, GeneralSecurityException::class)
    fun write(file: File, key: ByteArray) {
        val blob = wrap(key)
        val atomic = AtomicFile(file)
        val out = atomic.startWrite()
        try {
            out.write(blob)
            atomic.finishWrite(out)
        } catch (e: IOException) {
            atomic.failWrite(out)
            throw e
        }
    }

    /** Lit et désenveloppe la clé (à effacer par l'appelant). */
    @Throws(IOException::class, GeneralSecurityException::class)
    fun read(file: File): ByteArray {
        val blob = AtomicFile(file).readFully()
        return unwrap(blob)
    }

    /** Écrit puis relit : le Keystore doit fonctionner AVANT qu'on en dépende. */
    @Throws(IOException::class, GeneralSecurityException::class)
    fun writeChecked(file: File, key: ByteArray) {
        write(file, key)
        val back = try {
            read(file)
        } catch (e: Exception) {
            AtomicFile(file).delete()
            throw e
        }
        val same = back.contentEquals(key)
        back.fill(0)
        if (!same) {
            AtomicFile(file).delete()
            throw GeneralSecurityException("relecture du fichier de clé")
        }
    }

    /** Supprime le fichier (absent = déjà fait). `false` si impossible. */
    fun delete(file: File): Boolean {
        AtomicFile(file).delete()
        return !file.exists()
    }
}
