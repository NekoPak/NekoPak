package io.nekopak.desktop

import android.test.InstrumentationTestCase
import java.io.File
import java.security.MessageDigest
import java.util.UUID

/** Synthetic, unique records only. Never enumerates or loads account records. */
@Suppress("DEPRECATION")
class SessionVaultTest : InstrumentationTestCase() {
    private fun target() = "NekoPak/test/${UUID.randomUUID()}"

    private fun file(target: String): File {
        val hash = MessageDigest.getInstance("SHA-256").digest(target.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it.toInt() and 0xff) }
        return File(instrumentation.targetContext.noBackupFilesDir, "nekopak-vault-v1/$hash.sealed")
    }

    fun testRoundtripOverwriteAndDelete() {
        val context = instrumentation.targetContext
        val target = target()
        try {
            assertNull(SessionVault.load(context, target))
            val first = "synthetic-session-one".toByteArray()
            SessionVault.save(context, target, first)
            assertTrue(first.all { it == 0.toByte() })
            assertEquals("synthetic-session-one", SessionVault.load(context, target)!!.toString(Charsets.UTF_8))
            val ciphertext = file(target).readBytes()
            assertFalse(ciphertext.toString(Charsets.ISO_8859_1).contains("synthetic-session-one"))
            SessionVault.save(context, target, "synthetic-session-two".toByteArray())
            assertEquals("synthetic-session-two", SessionVault.load(context, target)!!.toString(Charsets.UTF_8))
            assertFalse(ciphertext.contentEquals(file(target).readBytes()))
            SessionVault.delete(context, target)
            assertNull(SessionVault.load(context, target))
            SessionVault.delete(context, target)
        } finally {
            SessionVault.delete(context, target)
        }
    }

    fun testCorruptionIsRejectedAndPreserved() {
        val context = instrumentation.targetContext
        val target = target()
        try {
            SessionVault.save(context, target, "synthetic-corruption-test".toByteArray())
            val record = file(target)
            val changed = record.readBytes().also { it[it.lastIndex] = (it.last().toInt() xor 1).toByte() }
            record.writeBytes(changed)
            var rejected = false
            try {
                SessionVault.load(context, target)
            } catch (exception: Exception) {
                rejected = true
            }
            assertTrue("Corrupt ciphertext must fail closed", rejected)
            assertTrue("Failed read must preserve corrupted evidence", changed.contentEquals(record.readBytes()))
            rejected = false
            val replacement = "must-not-replace-evidence".toByteArray()
            try {
                SessionVault.save(context, target, replacement)
            } catch (exception: Exception) {
                rejected = true
            }
            assertTrue("A write must not silently repair corruption", rejected)
            assertTrue(replacement.all { it == 0.toByte() })
            assertTrue(changed.contentEquals(record.readBytes()))
        } finally {
            SessionVault.delete(context, target)
        }
    }

    fun testCiphertextCannotMoveToAnotherTarget() {
        val context = instrumentation.targetContext
        val first = target()
        val second = target()
        try {
            SessionVault.save(context, first, "synthetic-account-one".toByteArray())
            file(second).writeBytes(file(first).readBytes())
            var rejected = false
            try {
                SessionVault.load(context, second)
            } catch (exception: Exception) {
                rejected = true
            }
            assertTrue("AAD must bind ciphertext to its account/auxiliary target", rejected)
            assertEquals("synthetic-account-one", SessionVault.load(context, first)!!.toString(Charsets.UTF_8))
        } finally {
            SessionVault.delete(context, first)
            SessionVault.delete(context, second)
        }
    }
}
