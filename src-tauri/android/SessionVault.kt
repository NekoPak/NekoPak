package io.nekopak.desktop

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.system.Os
import android.system.OsConstants
import android.util.AtomicFile
import java.io.File
import java.io.FileNotFoundException
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Native-only credential storage. The AES key never leaves Android Keystore;
 * noBackupFilesDir contains authenticated ciphertext, never session plaintext.
 * Called by Rust JNI workers, not a WebView interface or Tauri JS command.
 */
object SessionVault {
    private const val KEY_ALIAS = "io.nekopak.desktop.sessions.v1"
    private const val MAX_SECRET_BYTES = 65_536
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128
    private const val FORMAT = 1

    private fun checkedTarget(target: String): ByteArray {
        check(target.isNotEmpty() && target.length <= 1024 && !target.any { it.code < 32 }) {
            "Credential storage unavailable"
        }
        return target.toByteArray(Charsets.UTF_8)
    }

    private fun directory(context: Context): File {
        val directory = File(context.applicationContext.noBackupFilesDir, "nekopak-vault-v1")
        check(directory.isDirectory || directory.mkdir()) { "Credential storage unavailable" }
        return directory
    }

    private fun record(directory: File, target: ByteArray): AtomicFile {
        val name = MessageDigest.getInstance("SHA-256").digest(target)
            .joinToString("") { "%02x".format(it.toInt() and 0xff) }
        return AtomicFile(File(directory, "$name.sealed"))
    }

    private fun key(directory: File, create: Boolean): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (store.containsAlias(KEY_ALIAS)) {
            // Invalidated or inaccessible keys must propagate failure. Never
            // delete a key or silently replace the key sealing existing data.
            return store.getKey(KEY_ALIAS, null) as? SecretKey
                ?: error("Credential storage unavailable")
        }
        check(create && directory.list()?.isEmpty() == true) {
            "Credential storage unavailable"
        }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        return generator.generateKey()
    }

    private fun syncDirectory(directory: File) {
        val descriptor = Os.open(directory.path, OsConstants.O_RDONLY or OsConstants.O_NOFOLLOW, 0)
        try {
            check(OsConstants.S_ISDIR(Os.fstat(descriptor).st_mode)) { "Credential storage unavailable" }
            Os.fsync(descriptor)
        } finally {
            Os.close(descriptor)
        }
    }

    @JvmStatic
    @Synchronized
    fun load(context: Context, target: String): ByteArray? {
        val aad = checkedTarget(target)
        val directory = directory(context)
        val record = record(directory, aad)
        val payload = try {
            record.openRead().use { stream ->
                val length = stream.channel.size()
                check(length in (1L + IV_BYTES + TAG_BITS / 8 + 1)..(MAX_SECRET_BYTES.toLong() + 1 + IV_BYTES + TAG_BITS / 8)) {
                    "Credential storage unavailable"
                }
                // Reading a bounded record avoids allocating based on corrupt
                // or untrusted metadata. Check length again after the read.
                val bytes = ByteArray(length.toInt())
                var offset = 0
                while (offset < bytes.size) {
                    val count = stream.read(bytes, offset, bytes.size - offset)
                    check(count > 0) { "Credential storage unavailable" }
                    offset += count
                }
                check(stream.read() == -1) { "Credential storage unavailable" }
                bytes
            }
        } catch (exception: FileNotFoundException) {
            // A genuinely missing entry is a normal signed-out state. An
            // existing unreadable record or interrupted write fails closed.
            val base = record.baseFile
            if (base.exists() || File(base.path + ".bak").exists() || File(base.path + ".new").exists()) {
                throw IllegalStateException("Credential storage unavailable")
            }
            return null
        }
        check(payload[0].toInt() == FORMAT) { "Credential storage unavailable" }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(directory, create = false), GCMParameterSpec(TAG_BITS, payload, 1, IV_BYTES))
        cipher.updateAAD(aad)
        return cipher.doFinal(payload, 1 + IV_BYTES, payload.size - 1 - IV_BYTES).also {
            check(it.isNotEmpty() && it.size <= MAX_SECRET_BYTES) { "Credential storage unavailable" }
        }
    }

    @JvmStatic
    @Synchronized
    fun save(context: Context, target: String, value: ByteArray) {
        try {
            check(value.isNotEmpty() && value.size <= MAX_SECRET_BYTES) { "Credential storage unavailable" }
            val aad = checkedTarget(target)
            val directory = directory(context)
            // A write is not a repair operation. Refuse to overwrite a corrupt
            // entry or ciphertext whose OS key has become unavailable.
            load(context, target)?.fill(0)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key(directory, create = true))
            cipher.updateAAD(aad)
            check(cipher.iv.size == IV_BYTES) { "Credential storage unavailable" }
            val encrypted = cipher.doFinal(value)
            val payload = byteArrayOf(FORMAT.toByte()) + cipher.iv + encrypted
            val record = record(directory, aad)
            val output = record.startWrite()
            try {
                output.write(payload)
                // AtomicFile logs some sync/rename failures instead of throwing.
                // Explicitly sync, then verify the exact committed ciphertext.
                output.fd.sync()
                record.finishWrite(output)
            } catch (exception: Exception) {
                record.failWrite(output)
                throw IllegalStateException("Credential storage unavailable")
            }
            check(record.openRead().use { it.readBytes() }.contentEquals(payload)) {
                "Credential storage unavailable"
            }
            syncDirectory(directory)
        } finally {
            value.fill(0)
        }
    }

    @JvmStatic
    @Synchronized
    fun delete(context: Context, target: String) {
        val directory = directory(context)
        val record = record(directory, checkedTarget(target))
        record.delete()
        val base = record.baseFile
        check(!base.exists() && !File(base.path + ".bak").exists() && !File(base.path + ".new").exists()) {
            "Credential storage unavailable"
        }
        syncDirectory(directory)
        // Keep the key even after logout: other account/transfer records may
        // still need it, and same-install updates must retain its identity.
    }
}
