package io.github.varyen.detour

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import io.nekohasekai.libbox.Libbox
import java.io.File

/**
 * Единственная точка стыка ядра на Rust с андроидной частью.
 *
 * Ядро работает в том же процессе, но с собственных потоков: искать классы
 * приложения оттуда JNI не умеет (у примонтированного потока системный
 * загрузчик классов), поэтому Rust при старте запоминает ссылку на этот класс
 * и дальше зовёт его статические методы.
 */
object DetourBridge {
    init {
        System.loadLibrary("detour_app_lib")
    }

    /** Отдаёт ядру путь к движку обхода DPI из APK и забирает указатель на JVM. */
    @JvmStatic
    external fun nativeInit(dpiBinary: String)

    /** Пустая строка — туннель поднят, иначе текст ошибки для панели. */
    @JvmStatic
    fun tunnelStart(configPath: String): String = DetourVpn.start(configPath)

    @JvmStatic
    fun tunnelStop() = DetourVpn.stop()

    @JvmStatic
    fun tunnelRunning(): Boolean = DetourVpn.running()

    /** Пустая строка — конфиг валиден, иначе жалоба ядра. */
    @JvmStatic
    fun tunnelCheck(configPath: String): String = try {
        Libbox.checkConfig(java.io.File(configPath).readText())
        ""
    } catch (t: Throwable) {
        t.message ?: t.toString()
    }

    @JvmStatic
    fun tunnelVersion(): String = runCatching { Libbox.version() }.getOrDefault("")

    /**
     * Обновление приложения: системный диалог установки APK, который ядро
     * скачало и проверило по подписи. Пустая строка — диалог показан;
     * "need_permission" — открыта настройка «Установка неизвестных
     * приложений», после неё человек жмёт «Установить» ещё раз.
     */
    @JvmStatic
    fun installApk(path: String): String {
        val ctx = DetourVpn.context() ?: return "приложение ещё не запущено"
        return try { installApkWith(ctx, path) } catch (t: Throwable) { t.message ?: t.toString() }
    }

    private fun installApkWith(ctx: android.content.Context, path: String): String =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !ctx.packageManager.canRequestPackageInstalls()) {
            ctx.startActivity(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + ctx.packageName))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
            "need_permission"
        } else {
            // FileProvider отдаёт только кэш: файл ядра лежит в каталоге данных.
            val apk = File(ctx.cacheDir, "detour-update.apk")
            File(path).copyTo(apk, overwrite = true)
            val uri = FileProvider.getUriForFile(ctx, ctx.packageName + ".fileprovider", apk)
            ctx.startActivity(
                Intent(Intent.ACTION_VIEW)
                    .setDataAndType(uri, "application/vnd.android.package-archive")
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            )
            ""
        }
}
