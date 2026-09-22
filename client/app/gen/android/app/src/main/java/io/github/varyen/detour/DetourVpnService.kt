package io.github.varyen.detour

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.IpPrefix
import android.net.Network
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import io.nekohasekai.libbox.CommandServer
import io.nekohasekai.libbox.CommandServerHandler
import io.nekohasekai.libbox.ConnectionOwner
import io.nekohasekai.libbox.ExchangeContext
import io.nekohasekai.libbox.InterfaceUpdateListener
import io.nekohasekai.libbox.Libbox
import io.nekohasekai.libbox.LocalDNSTransport
import io.nekohasekai.libbox.NetworkInterfaceIterator
import io.nekohasekai.libbox.OverrideOptions
import io.nekohasekai.libbox.PlatformInterface
import io.nekohasekai.libbox.RoutePrefix
import io.nekohasekai.libbox.RoutePrefixIterator
import io.nekohasekai.libbox.SetupOptions
import io.nekohasekai.libbox.StringIterator
import io.nekohasekai.libbox.SystemProxyStatus
import io.nekohasekai.libbox.TunOptions
import io.nekohasekai.libbox.WIFIState
import io.nekohasekai.libbox.NetworkInterface as BoxInterface
import java.io.File
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NetworkInterface as JavaInterface
import java.net.UnknownHostException

/**
 * Туннель Detour на Android. Ядро sing-box живёт здесь же, в процессе
 * приложения (libbox), а не отдельным бинарником: дескриптор TUN выдаёт сама
 * система через `VpnService.Builder`, и передать его чужому процессу нельзя.
 */
class DetourVpnService : VpnService(), PlatformInterface, CommandServerHandler {
    @Volatile
    var tunnelUp: Boolean = false
        private set

    private var server: CommandServer? = null
    private var pfd: ParcelFileDescriptor? = null
    private var monitor: ConnectivityManager.NetworkCallback? = null
    private var configPath: String? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == DetourVpn.ACTION_STOP) {
            shutdown()
            return START_NOT_STICKY
        }
        goForeground()
        val path = intent?.getStringExtra(DetourVpn.EXTRA_CONFIG)
        Thread({ boot(path) }, "detour-tunnel").start()
        return START_NOT_STICKY
    }

    private fun boot(path: String?) {
        try {
            val file = path?.let(::File) ?: error("не передан путь к конфигу")
            configPath = path
            val content = file.readText()
            setup(this)
            val srv = Libbox.newCommandServer(this, this)
            // Командный сервер нужен только сторонним клиентам; панель ходит в
            // ядро напрямую, поэтому его падение туннелю не мешает.
            runCatching { srv.start() }.onFailure { Log.w(TAG, "командный сервер", it) }
            srv.startOrReloadService(content, OverrideOptions())
            server = srv
            tunnelUp = true
            DetourVpn.started(this)
        } catch (t: Throwable) {
            Log.e(TAG, "туннель не поднялся", t)
            closeTunnel()
            DetourVpn.failed(t.message ?: t.toString())
            stopSelf()
        }
    }

    fun shutdown() {
        closeTunnel()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onRevoke() = shutdown()

    override fun onDestroy() {
        closeTunnel()
        super.onDestroy()
    }

    private fun closeTunnel() {
        tunnelUp = false
        server?.let { srv ->
            runCatching { srv.closeService() }
            runCatching { srv.close() }
        }
        server = null
        monitor?.let { cb ->
            runCatching { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(cb) }
        }
        monitor = null
        runCatching { pfd?.close() }
        pfd = null
        DetourVpn.detach(this)
    }

    private fun goForeground() {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, "Туннель", NotificationManager.IMPORTANCE_LOW)
            )
        }
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        val notification = builder
            .setContentTitle("Detour")
            .setContentText("Туннель работает")
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setOngoing(true)
            .setContentIntent(open)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    // --- PlatformInterface ---

    override fun openTun(options: TunOptions): Int {
        check(prepare(this) == null) { "нет разрешения на VPN" }
        val builder = Builder().setSession("Detour").setMtu(options.getMTU())
        options.getInet4Address().forEach { builder.addAddress(it.address(), it.prefix()) }
        options.getInet6Address().forEach { builder.addAddress(it.address(), it.prefix()) }

        if (options.getAutoRoute()) {
            runCatching { builder.addDnsServer(options.getDNSServerAddress().value) }
                .onFailure { Log.w(TAG, "DNS туннеля", it) }
            val v4 = options.getInet4RouteAddress()
            if (v4.hasNext()) v4.forEach { builder.addRoute(it.address(), it.prefix()) }
            else builder.addRoute("0.0.0.0", 0)
            val v6 = options.getInet6RouteAddress()
            if (v6.hasNext()) v6.forEach { builder.addRoute(it.address(), it.prefix()) }
            else builder.addRoute("::", 0)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                options.getInet4RouteExcludeAddress().forEach {
                    builder.excludeRoute(IpPrefix(InetAddress.getByName(it.address()), it.prefix()))
                }
                options.getInet6RouteExcludeAddress().forEach {
                    builder.excludeRoute(IpPrefix(InetAddress.getByName(it.address()), it.prefix()))
                }
            }
        }

        val include = options.getIncludePackage().toList()
        if (include.isEmpty()) {
            // Своё приложение — мимо туннеля. Иначе tpws, который ходит наружу
            // сам, вернулся бы в sing-box тем же правилом обхода: петля.
            builder.addDisallowedApplication(packageName)
            options.getExcludePackage().forEach {
                runCatching { builder.addDisallowedApplication(it) }
            }
        } else {
            include.forEach { runCatching { builder.addAllowedApplication(it) } }
        }

        val fd = builder.establish() ?: error("система не отдала дескриптор TUN")
        pfd = fd
        return fd.fd
    }

    override fun usePlatformAutoDetectInterfaceControl(): Boolean = true

    override fun autoDetectInterfaceControl(fd: Int) {
        check(protect(fd)) { "не удалось вывести сокет $fd из туннеля" }
    }

    override fun useProcFS(): Boolean = Build.VERSION.SDK_INT < Build.VERSION_CODES.Q

    override fun findConnectionOwner(
        ipProtocol: Int,
        sourceAddress: String,
        sourcePort: Int,
        destinationAddress: String,
        destinationPort: Int,
    ): ConnectionOwner {
        check(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) { "поиск владельца недоступен" }
        val uid = getSystemService(ConnectivityManager::class.java).getConnectionOwnerUid(
            ipProtocol,
            InetSocketAddress(InetAddress.getByName(sourceAddress), sourcePort),
            InetSocketAddress(InetAddress.getByName(destinationAddress), destinationPort),
        )
        check(uid != -1) { "владелец соединения не найден" }
        val owner = ConnectionOwner()
        owner.userId = uid
        packageManager.getPackagesForUid(uid)?.let { owner.setAndroidPackageNames(StringArray(it.toList())) }
        packageManager.getNameForUid(uid)?.let { owner.userName = it }
        return owner
    }

    /* Подписка на смену сети — только повод пересчитать: «основной» сетью
       система с поднятым туннелем называет сам туннель, а ядру нужен тот
       интерфейс, через который туннель выходит наружу. */
    override fun startDefaultInterfaceMonitor(listener: InterfaceUpdateListener) {
        val manager = getSystemService(ConnectivityManager::class.java)
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) = update()

            override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) = update()

            override fun onLost(network: Network) = update()

            private fun update() {
                val net = physical(manager)
                val name = net?.let { manager.getLinkProperties(it)?.interfaceName }
                val index = name?.let { runCatching { JavaInterface.getByName(it)?.index }.getOrNull() }
                if (net == null || name == null || index == null) {
                    listener.updateDefaultInterface("", -1, false, false)
                    return
                }
                val caps = manager.getNetworkCapabilities(net)
                val metered = caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == false
                listener.updateDefaultInterface(name, index, metered, false)
            }
        }
        manager.registerDefaultNetworkCallback(callback)
        monitor = callback
    }

    override fun closeDefaultInterfaceMonitor(listener: InterfaceUpdateListener) {
        monitor?.let {
            runCatching { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) }
        }
        monitor = null
    }

    override fun getInterfaces(): NetworkInterfaceIterator {
        val manager = getSystemService(ConnectivityManager::class.java)
        val active = physical(manager)
        val activeName = active?.let { manager.getLinkProperties(it)?.interfaceName }
        val caps = active?.let { manager.getNetworkCapabilities(it) }
        val items = ArrayList<BoxInterface>()
        for (nif in JavaInterface.getNetworkInterfaces()) {
            val item = BoxInterface()
            item.name = nif.name
            item.index = nif.index
            item.mtu = runCatching { nif.mtu }.getOrDefault(1500)
            item.flags = rawFlags(nif)
            item.addresses = StringArray(nif.interfaceAddresses.mapNotNull {
                val host = it.address?.hostAddress ?: return@mapNotNull null
                host.substringBefore('%') + "/" + it.networkPrefixLength
            })
            item.type = if (nif.name == activeName) typeOf(caps) else Libbox.InterfaceTypeOther
            item.metered = nif.name == activeName &&
                caps?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == false
            items.add(item)
        }
        return InterfaceArray(items)
    }

    /* Без этого ядро резолвит «локальные» домены сама Go-рантайм: она ищет
       /etc/resolv.conf, не находит и стучится в ::1:53. Отдаём ей системный
       резолвер — он у нашего приложения смотрит в физическую сеть, потому что
       из туннеля мы себя исключили. */
    override fun localDNSTransport(): LocalDNSTransport = object : LocalDNSTransport {
        override fun raw(): Boolean = false

        override fun lookup(ctx: ExchangeContext, network: String, domain: String) {
            try {
                val found = InetAddress.getAllByName(domain.trimEnd('.')).filter {
                    if (network == "ip6") it is Inet6Address else it is Inet4Address
                }
                ctx.success(found.mapNotNull { it.hostAddress }.joinToString(separator = "\n"))
            } catch (e: UnknownHostException) {
                ctx.errorCode(RCODE_NXDOMAIN)
            } catch (e: Exception) {
                Log.w(TAG, "резолв $domain", e)
                ctx.errorCode(RCODE_SERVFAIL)
            }
        }

        override fun exchange(ctx: ExchangeContext, message: ByteArray) {
            ctx.errorCode(RCODE_SERVFAIL)
        }
    }

    override fun readWIFIState(): WIFIState? = null

    override fun systemCertificates(): StringIterator? = null

    override fun underNetworkExtension(): Boolean = false

    override fun includeAllNetworks(): Boolean = false

    override fun clearDNSCache() {}

    override fun sendNotification(notification: io.nekohasekai.libbox.Notification) {}

    // --- CommandServerHandler ---

    override fun serviceStop() = shutdown()

    override fun serviceReload() {
        val path = configPath ?: return
        server?.startOrReloadService(File(path).readText(), OverrideOptions())
    }

    override fun getSystemProxyStatus(): SystemProxyStatus = SystemProxyStatus()

    override fun setSystemProxyEnabled(isEnabled: Boolean) {}

    override fun writeDebugMessage(message: String) {
        Log.d(TAG, message)
    }

    private companion object {
        const val TAG = "detour"
        const val CHANNEL = "detour-tunnel"
        const val NOTIFICATION_ID = 1
        const val RCODE_SERVFAIL = 2
        const val RCODE_NXDOMAIN = 3

        // Флаги интерфейса libbox ждёт сырыми, в виде IFF_* ядра Linux.
        const val IFF_UP = 1
        const val IFF_BROADCAST = 2
        const val IFF_LOOPBACK = 8
        const val IFF_POINTOPOINT = 0x10
        const val IFF_RUNNING = 0x40
        const val IFF_MULTICAST = 0x1000

        var configured = false

        @Synchronized
        fun setup(ctx: Context) {
            if (configured) return
            val options = SetupOptions()
            options.basePath = ctx.filesDir.absolutePath
            options.workingPath = File(ctx.filesDir, "sing-box").absolutePath
            options.tempPath = ctx.cacheDir.absolutePath
            options.fixAndroidStack = true
            options.logMaxLines = 200
            Libbox.setup(options)
            runCatching { Libbox.redirectStderr(File(ctx.filesDir, "sing-box-panic.log").absolutePath) }
            configured = true
        }

        fun rawFlags(nif: JavaInterface): Int {
            var flags = 0
            if (nif.isUp) flags = flags or IFF_UP or IFF_RUNNING
            if (nif.isLoopback) flags = flags or IFF_LOOPBACK
            if (nif.isPointToPoint) flags = flags or IFF_POINTOPOINT
            if (nif.supportsMulticast()) flags = flags or IFF_MULTICAST
            if (!nif.isLoopback && !nif.isPointToPoint) flags = flags or IFF_BROADCAST
            return flags
        }

        /* Сеть, через которую туннель выходит наружу. Спросить систему
           «какая сейчас основная» нельзя: с поднятым VpnService основной она
           называет сам туннель, а подложка (getUnderlyingNetworks) закрыта от
           приложений. Поэтому перебираем сети сами: сначала проверенные,
           потом безлимитные. */
        @Suppress("DEPRECATION")
        fun physical(manager: ConnectivityManager): Network? {
            val active = manager.activeNetwork
            if (active != null && manager.getNetworkCapabilities(active)
                    ?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == false
            ) {
                return active
            }
            return manager.allNetworks
                .mapNotNull { net -> manager.getNetworkCapabilities(net)?.let { net to it } }
                .filter { (_, caps) ->
                    caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                        !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
                }
                .maxByOrNull { (_, caps) ->
                    var rank = 0
                    if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)) rank += 2
                    if (caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)) rank += 1
                    rank
                }
                ?.first
        }

        fun typeOf(caps: NetworkCapabilities?): Int = when {
            caps == null -> Libbox.InterfaceTypeOther
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> Libbox.InterfaceTypeWIFI
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> Libbox.InterfaceTypeCellular
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> Libbox.InterfaceTypeEthernet
            else -> Libbox.InterfaceTypeOther
        }
    }
}

private class StringArray(private val items: List<String>) : StringIterator {
    private var at = 0
    override fun len(): Int = items.size
    override fun hasNext(): Boolean = at < items.size
    override fun next(): String = items[at++]
}

private class InterfaceArray(private val items: List<BoxInterface>) : NetworkInterfaceIterator {
    private var at = 0
    override fun hasNext(): Boolean = at < items.size
    override fun next(): BoxInterface = items[at++]
}

private inline fun RoutePrefixIterator.forEach(action: (RoutePrefix) -> Unit) {
    while (hasNext()) action(next())
}

private inline fun StringIterator.forEach(action: (String) -> Unit) {
    while (hasNext()) action(next())
}

private fun StringIterator.toList(): List<String> {
    val items = ArrayList<String>()
    forEach { items.add(it) }
    return items
}
