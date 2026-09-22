package io.github.varyen.detour

import io.nekohasekai.libbox.Libbox

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
}
