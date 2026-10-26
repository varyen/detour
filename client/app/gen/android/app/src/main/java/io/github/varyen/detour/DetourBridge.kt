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

    /**
     * Приложения с доступом в сеть — для правил «приложение мимо VPN / через
     * VPN». JSON-массив `{match, name, system, icon}`; иконка — PNG 48×48 в
     * base64. «Системное» — без значка в лаунчере: службы, которые человек
     * сам не открывает.
     */
    @JvmStatic
    fun installedApps(): String {
        val ctx = DetourVpn.context() ?: return ""
        return try { appsJson(ctx) } catch (t: Throwable) { "" }
    }

    private fun appsJson(ctx: android.content.Context): String {
        val pm = ctx.packageManager
        val out = org.json.JSONArray()
        @Suppress("DEPRECATION")
        val pkgs = pm.getInstalledPackages(android.content.pm.PackageManager.GET_PERMISSIONS)
        for (p in pkgs) {
            val ai = p.applicationInfo ?: continue
            if (p.packageName == ctx.packageName) continue
            if (p.requestedPermissions?.contains(android.Manifest.permission.INTERNET) != true) continue
            val launcher = pm.getLaunchIntentForPackage(p.packageName) != null
            val o = org.json.JSONObject()
            o.put("match", p.packageName)
            o.put("name", ai.loadLabel(pm).toString())
            if (!launcher) o.put("system", true)
            if (launcher) runCatching { o.put("icon", iconPng(ai.loadIcon(pm))) }
            out.put(o)
        }
        return out.toString()
    }

    private fun iconPng(d: android.graphics.drawable.Drawable): String {
        val px = 48
        val bmp = android.graphics.Bitmap.createBitmap(px, px, android.graphics.Bitmap.Config.ARGB_8888)
        val c = android.graphics.Canvas(bmp)
        d.setBounds(0, 0, px, px)
        d.draw(c)
        val buf = java.io.ByteArrayOutputStream()
        bmp.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, buf)
        bmp.recycle()
        return android.util.Base64.encodeToString(buf.toByteArray(), android.util.Base64.NO_WRAP)
    }

    @JvmStatic
    fun tunnelVersion(): String = runCatching { Libbox.version() }.getOrDefault("")

    /**
     * Файл из панели (экспорт настроек, конфиг клиента): WebView загрузки
     * blob-ссылок молча игнорирует. Android 10+ — в «Загрузки/Detour» без
     * разрешений; раньше туда без WRITE_EXTERNAL_STORAGE не пишут, поэтому
     * системный диалог «Поделиться». Ответ "ok:<куда>" или текст ошибки.
     */
    @JvmStatic
    fun saveFile(name: String, content: String, mime: String): String {
        val ctx = DetourVpn.context() ?: return "приложение ещё не запущено"
        return try { saveFileWith(ctx, name, content.toByteArray(), mime) } catch (t: Throwable) { t.message ?: t.toString() }
    }

    private fun saveFileWith(ctx: android.content.Context, name: String, data: ByteArray, mime: String): String {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val dir = android.os.Environment.DIRECTORY_DOWNLOADS + "/Detour"
            val values = android.content.ContentValues().apply {
                put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME, name)
                put(android.provider.MediaStore.MediaColumns.MIME_TYPE, mime)
                put(android.provider.MediaStore.MediaColumns.RELATIVE_PATH, dir)
            }
            val resolver = ctx.contentResolver
            val uri = resolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                ?: return "система не дала создать файл в «Загрузках»"
            resolver.openOutputStream(uri)?.use { it.write(data) } ?: return "не удалось открыть файл на запись"
            return "ok:$dir/$name"
        }
        val out = File(ctx.cacheDir, name)
        out.writeBytes(data)
        val uri = FileProvider.getUriForFile(ctx, ctx.packageName + ".fileprovider", out)
        val send = Intent(Intent.ACTION_SEND)
            .setType(mime)
            .putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        ctx.startActivity(Intent.createChooser(send, name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        return "ok:share"
    }

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
