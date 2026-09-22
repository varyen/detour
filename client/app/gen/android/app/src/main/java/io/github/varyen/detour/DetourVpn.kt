package io.github.varyen.detour

import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Build
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Посредник между ядром и службой VPN: ядро зовёт `start`/`stop` синхронно и
 * ждёт результата, а поднимает туннель отдельная служба — иначе система не
 * отдаст файловый дескриптор TUN.
 */
object DetourVpn {
    const val ACTION_STOP = "io.github.varyen.detour.STOP"
    const val EXTRA_CONFIG = "config"

    /** Ждём дольше обычного: первый старт тянет geo-правила и поднимает gVisor. */
    private const val START_TIMEOUT_SEC = 25L

    @Volatile private var appContext: Context? = null
    @Volatile private var service: DetourVpnService? = null
    @Volatile private var lastError: String = ""
    @Volatile private var pending: CountDownLatch? = null

    /** Активити кладёт сюда способ показать системный запрос на VPN. */
    @Volatile var askPermission: (() -> Unit)? = null

    fun attach(context: Context) {
        appContext = context.applicationContext
    }

    fun running(): Boolean = service?.tunnelUp == true

    fun start(configPath: String): String {
        val ctx = appContext ?: return "приложение ещё не запущено"
        if (VpnService.prepare(ctx) != null) {
            askPermission?.invoke()
            return "нет разрешения на VPN: подтвердите запрос системы и повторите"
        }
        service?.shutdown()
        val latch = CountDownLatch(1)
        synchronized(this) {
            pending = latch
            lastError = ""
        }
        val intent = Intent(ctx, DetourVpnService::class.java).putExtra(EXTRA_CONFIG, configPath)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ctx.startForegroundService(intent)
            } else {
                ctx.startService(intent)
            }
        } catch (t: Throwable) {
            return t.message ?: "не удалось запустить службу VPN"
        }
        if (!latch.await(START_TIMEOUT_SEC, TimeUnit.SECONDS)) {
            return "туннель не поднялся за $START_TIMEOUT_SEC с"
        }
        return lastError
    }

    fun stop() {
        service?.shutdown()
        for (i in 0 until 100) {
            if (!running()) return
            Thread.sleep(50)
        }
    }

    internal fun started(vpn: DetourVpnService) = report(vpn, "")

    internal fun failed(error: String) = report(null, error)

    internal fun detach(vpn: DetourVpnService) {
        if (service === vpn) service = null
    }

    private fun report(vpn: DetourVpnService?, error: String) {
        val latch = synchronized(this) {
            service = vpn
            lastError = error
            pending.also { pending = null }
        }
        latch?.countDown()
    }
}
