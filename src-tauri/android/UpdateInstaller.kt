package io.nekopak.desktop

import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File
import java.security.MessageDigest

class UpdateFileProvider : FileProvider()

/** Native-only handoff of a verified package. No JavaScript path or URL is accepted. */
object UpdateInstaller {
    @Suppress("DEPRECATION")
    private fun signatures(info: PackageInfo): Set<String> {
        val values = if (Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners else info.signatures
        return values?.map { signature ->
            MessageDigest.getInstance("SHA-256").digest(signature.toByteArray()).joinToString("") { "%02x".format(it.toInt() and 255) }
        }?.toSet() ?: emptySet()
    }

    @JvmStatic
    @Suppress("DEPRECATION")
    fun install(context: Context, path: String, sha256: String, version: String, length: Long): Int {
        val directory = File(context.filesDir, "updates").canonicalFile
        val file = File(path)
        check(file.canonicalFile == file.absoluteFile && file.parentFile == directory && file.extension == "apk")
        check(file.isFile && file.length() == length && length in 1..(256L * 1024 * 1024))
        val hash = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(65536)
            while (true) { val count = input.read(buffer); if (count < 0) break; hash.update(buffer, 0, count) }
        }
        check(hash.digest().joinToString("") { "%02x".format(it.toInt() and 255) } == sha256)
        val flags = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES
        val installed = context.packageManager.getPackageInfo(context.packageName, flags)
        val candidate = context.packageManager.getPackageArchiveInfo(path, flags) ?: error("Invalid package")
        check(candidate.packageName == context.packageName && candidate.versionName == version)
        val oldCode = if (Build.VERSION.SDK_INT >= 28) installed.longVersionCode else installed.versionCode.toLong()
        val newCode = if (Build.VERSION.SDK_INT >= 28) candidate.longVersionCode else candidate.versionCode.toLong()
        check(newCode > oldCode && signatures(installed).isNotEmpty() && signatures(installed) == signatures(candidate))
        if (Build.VERSION.SDK_INT >= 26 && !context.packageManager.canRequestPackageInstalls()) {
            context.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            return 1
        }
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", file)
        context.startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION))
        return 0
    }
}
