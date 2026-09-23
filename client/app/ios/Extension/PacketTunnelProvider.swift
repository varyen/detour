// Туннель Detour на iOS. НЕ СОБИРАЛОСЬ (нет Xcode с iOS SDK) — см.
// client/app/ios/README.md.
//
// Отдельный процесс-расширение: система отдаёт TUN только ему. Внутри — libbox
// (sing-box), тот же, что на Android, и то же устройство PlatformInterface
// (client/app/gen/android/.../DetourVpnService.kt). Отличия от Android:
//  * дескриптор TUN libbox находит сам (GetTunnelFileDescriptor) — достаточно
//    применить сетевые настройки;
//  * собственные сокеты расширения идут мимо туннеля сами, «защищать» их, как
//    VpnService.protect(), не нужно;
//  * обхода DPI нет: запускать сторонние процессы iOS не даёт.

import Foundation
import Libbox
import Network
import NetworkExtension

final class PacketTunnelProvider: NEPacketTunnelProvider {
    private var server: LibboxCommandServer?
    private var platform: Platform?

    override func startTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        guard let config = options?["config"] as? String else {
            completionHandler(failure("не передан конфиг"))
            return
        }
        do {
            try Self.setupOnce()
            let platform = Platform(provider: self)
            var error: NSError?
            guard let srv = LibboxNewCommandServer(platform, platform, &error) else {
                throw error ?? failure("libbox не создал сервер")
            }
            try srv.startOrReloadService(config, options: LibboxOverrideOptions())
            server = srv
            self.platform = platform
            completionHandler(nil)
        } catch {
            NSLog("detour: туннель не поднялся: \(error)")
            completionHandler(error)
        }
    }

    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        try? server?.closeService()
        server?.close()
        server = nil
        platform?.stopMonitor()
        platform = nil
        completionHandler()
    }

    private static var configured = false

    private static func setupOnce() throws {
        if configured { return }
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Detour", isDirectory: true)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        let options = LibboxSetupOptions()
        options.basePath = base.path
        options.workingPath = base.appendingPathComponent("sing-box").path
        options.tempPath = NSTemporaryDirectory()
        options.logMaxLines = 200
        var error: NSError?
        LibboxSetup(options, &error)
        if let error { throw error }
        configured = true
    }
}

func failure(_ text: String) -> NSError {
    NSError(domain: "detour", code: 1, userInfo: [NSLocalizedDescriptionKey: text])
}

// MARK: - PlatformInterface

final class Platform: NSObject, LibboxPlatformInterfaceProtocol, LibboxCommandServerHandlerProtocol {
    private weak var provider: NEPacketTunnelProvider?
    private var monitor: NWPathMonitor?
    private var path: Network.NWPath?
    private let queue = DispatchQueue(label: "detour.path")

    init(provider: NEPacketTunnelProvider) {
        self.provider = provider
    }

    func stopMonitor() {
        monitor?.cancel()
        monitor = nil
    }

    func openTun(_ options: LibboxTunOptionsProtocol?, ret0_: UnsafeMutablePointer<Int32>?) throws {
        guard let options, let ret0_, let provider else { throw failure("нет параметров TUN") }
        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "127.0.0.1")
        settings.mtu = NSNumber(value: options.getMTU())

        var v4: [String] = [], v4mask: [String] = []
        each(options.getInet4Address()) { v4.append($0.address()); v4mask.append($0.mask()) }
        let ipv4 = NEIPv4Settings(addresses: v4, subnetMasks: v4mask)

        var v6: [String] = [], v6len: [NSNumber] = []
        each(options.getInet6Address()) { v6.append($0.address()); v6len.append(NSNumber(value: $0.prefix())) }
        let ipv6 = v6.isEmpty ? nil : NEIPv6Settings(addresses: v6, networkPrefixLengths: v6len)

        if options.getAutoRoute() {
            ipv4.includedRoutes = routes4(options.getInet4RouteAddress()) ?? [NEIPv4Route.default()]
            ipv4.excludedRoutes = routes4(options.getInet4RouteExcludeAddress()) ?? []
            ipv6?.includedRoutes = routes6(options.getInet6RouteAddress()) ?? [NEIPv6Route.default()]
            ipv6?.excludedRoutes = routes6(options.getInet6RouteExcludeAddress()) ?? []
            if let dns = try? options.getDNSServerAddress().value {
                let d = NEDNSSettings(servers: [dns])
                // Пустой домен — «все запросы сюда», иначе iOS шлёт в туннель
                // только часть имён.
                d.matchDomains = [""]
                settings.dnsSettings = d
            }
        }
        settings.ipv4Settings = ipv4
        settings.ipv6Settings = ipv6

        let done = DispatchSemaphore(value: 0)
        var applyError: Error?
        provider.setTunnelNetworkSettings(settings) { error in
            applyError = error
            done.signal()
        }
        done.wait()
        if let applyError { throw applyError }

        let fd = LibboxGetTunnelFileDescriptor()
        guard fd >= 0 else { throw failure("система не отдала дескриптор TUN") }
        ret0_.pointee = fd
    }

    // Сокеты расширения и так не заходят в собственный туннель.
    func usePlatformAutoDetectInterfaceControl() -> Bool { true }
    func autoDetectInterfaceControl(_ fd: Int32) throws {}

    func underNetworkExtension() -> Bool { true }
    func includeAllNetworks() -> Bool { false }
    func useProcFS() -> Bool { false }

    func findConnectionOwner(_ ipProtocol: Int32, sourceAddress: String?, sourcePort: Int32,
                             destinationAddress: String?, destinationPort: Int32) throws -> LibboxConnectionOwner {
        throw failure("владельца соединения iOS не сообщает")
    }

    func startDefaultInterfaceMonitor(_ listener: LibboxInterfaceUpdateListenerProtocol?) throws {
        guard let listener else { return }
        let monitor = NWPathMonitor()
        let first = DispatchSemaphore(value: 0)
        var reported = false
        monitor.pathUpdateHandler = { [weak self] path in
            self?.path = path
            Self.report(path, to: listener)
            if !reported {
                reported = true
                first.signal()
            }
        }
        monitor.start(queue: queue)
        // libbox ждёт первый ответ до того, как начнёт открывать соединения.
        _ = first.wait(timeout: .now() + 5)
        self.monitor = monitor
    }

    func closeDefaultInterfaceMonitor(_ listener: LibboxInterfaceUpdateListenerProtocol?) throws {
        stopMonitor()
    }

    private static func physical(_ path: Network.NWPath) -> [NWInterface] {
        path.availableInterfaces.filter { !$0.name.hasPrefix("utun") }
    }

    private static func report(_ path: Network.NWPath, to listener: LibboxInterfaceUpdateListenerProtocol) {
        guard path.status == .satisfied, let iface = physical(path).first else {
            listener.updateDefaultInterface("", interfaceIndex: -1, isExpensive: false, isConstrained: false)
            return
        }
        listener.updateDefaultInterface(iface.name, interfaceIndex: Int32(iface.index),
                                        isExpensive: path.isExpensive, isConstrained: path.isConstrained)
    }

    /// На Apple libbox добирает адреса и флаги сам по индексу — хватает имени,
    /// индекса и типа.
    func getInterfaces() throws -> LibboxNetworkInterfaceIteratorProtocol {
        let list = path.map(Self.physical) ?? []
        return InterfaceArray(list.map { nif in
            let item = LibboxNetworkInterface()
            item.name = nif.name
            item.index = Int32(nif.index)
            switch nif.type {
            case .wifi: item.type = LibboxInterfaceTypeWIFI
            case .cellular: item.type = LibboxInterfaceTypeCellular
            case .wiredEthernet: item.type = LibboxInterfaceTypeEthernet
            default: item.type = LibboxInterfaceTypeOther
            }
            return item
        })
    }

    func localDNSTransport() -> LibboxLocalDNSTransportProtocol? { nil }
    func readWIFIState() -> LibboxWIFIState? { nil }
    func systemCertificates() -> LibboxStringIteratorProtocol? { nil }
    func clearDNSCache() {}
    func send(_ notification: LibboxNotification?) throws {}

    // MARK: CommandServerHandler

    func serviceStop() throws {
        provider?.cancelTunnelWithError(nil)
    }

    func serviceReload() throws {}

    func getSystemProxyStatus() throws -> LibboxSystemProxyStatus {
        LibboxSystemProxyStatus()
    }

    func setSystemProxyEnabled(_ isEnabled: Bool) throws {}

    func writeDebugMessage(_ message: String?) {
        NSLog("detour: \(message ?? "")")
    }
}

private final class InterfaceArray: NSObject, LibboxNetworkInterfaceIteratorProtocol {
    private var items: [LibboxNetworkInterface]
    init(_ items: [LibboxNetworkInterface]) { self.items = items }
    func hasNext() -> Bool { !items.isEmpty }
    func next() -> LibboxNetworkInterface? { items.isEmpty ? nil : items.removeFirst() }
}

private func each(_ it: LibboxRoutePrefixIteratorProtocol?, _ body: (LibboxRoutePrefix) -> Void) {
    guard let it else { return }
    while it.hasNext() {
        if let p = it.next() { body(p) }
    }
}

private func routes4(_ it: LibboxRoutePrefixIteratorProtocol?) -> [NEIPv4Route]? {
    var out: [NEIPv4Route] = []
    each(it) { out.append(NEIPv4Route(destinationAddress: $0.address(), subnetMask: $0.mask())) }
    return out.isEmpty ? nil : out
}

private func routes6(_ it: LibboxRoutePrefixIteratorProtocol?) -> [NEIPv6Route]? {
    var out: [NEIPv6Route] = []
    each(it) { out.append(NEIPv6Route(destinationAddress: $0.address(), networkPrefixLength: NSNumber(value: $0.prefix()))) }
    return out.isEmpty ? nil : out
}
