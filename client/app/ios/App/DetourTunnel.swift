// Управление туннелем из приложения. НЕ СОБИРАЛОСЬ (нет Xcode с iOS SDK) —
// см. client/app/ios/README.md.
//
// Туннель живёт в расширении DetourTunnel (NEPacketTunnelProvider), приложение
// только включает и выключает его через NETunnelProviderManager. Ядро на Rust
// зовёт эти функции со своего рабочего потока (client/app/src/ios.rs), поэтому
// асинхронные вызовы NetworkExtension сведены здесь к синхронным.

import Foundation
import Libbox
import NetworkExtension

enum DetourTunnel {
    static let extensionBundleID = "io.github.varyen.detour.tunnel"
    /// Первый старт тянет geo-правила и поднимает gVisor — как и на Android,
    /// ждём дольше обычного.
    static let startTimeout: TimeInterval = 25

    private static var cached: NETunnelProviderManager?
    private static let lock = NSLock()

    /// Профиль VPN в системе. Первое сохранение показывает системный запрос
    /// «Detour хочет добавить конфигурацию VPN» — без согласия дальше не пройти.
    static func manager() throws -> NETunnelProviderManager {
        lock.lock()
        defer { lock.unlock() }
        if let m = cached { return m }
        let all: [NETunnelProviderManager] = try blocking { done in
            NETunnelProviderManager.loadAllFromPreferences { list, error in done(list ?? [], error) }
        }
        let m = all.first ?? NETunnelProviderManager()
        let proto = (m.protocolConfiguration as? NETunnelProviderProtocol) ?? NETunnelProviderProtocol()
        proto.providerBundleIdentifier = extensionBundleID
        proto.serverAddress = "Detour"
        m.protocolConfiguration = proto
        m.localizedDescription = "Detour"
        m.isEnabled = true
        try blocking { done in m.saveToPreferences { done((), $0) } }
        // Без перечитывания только что сохранённый профиль не стартует
        // (известная особенность NETunnelProviderManager).
        try blocking { done in m.loadFromPreferences { done((), $0) } }
        cached = m
        return m
    }

    /// nil — туннель поднят, иначе текст ошибки для панели.
    static func start(config: String) -> String? {
        do {
            let m = try manager()
            let connection = m.connection
            if connection.status != .disconnected && connection.status != .invalid {
                connection.stopVPNTunnel()
                _ = waitStatus(connection, until: [.disconnected, .invalid], timeout: 10)
            }
            // Конфиг — параметром запуска: у расширения своя песочница, файлы
            // приложения ему не видны.
            try connection.startVPNTunnel(options: ["config": config as NSString])
            let final = waitStatus(connection, until: [.connected, .disconnected, .invalid], timeout: startTimeout)
            switch final {
            case .connected:
                return nil
            case .disconnected, .invalid:
                return lastError(connection) ?? "туннель не поднялся"
            default:
                return "туннель не поднялся за \(Int(startTimeout)) с"
            }
        } catch {
            return error.localizedDescription
        }
    }

    static func stop() {
        guard let m = try? manager() else { return }
        m.connection.stopVPNTunnel()
        _ = waitStatus(m.connection, until: [.disconnected, .invalid], timeout: 5)
    }

    static func running() -> Bool {
        (try? manager())?.connection.status == .connected
    }

    private static func waitStatus(_ c: NEVPNConnection, until: Set<NEVPNStatus>, timeout: TimeInterval) -> NEVPNStatus {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if until.contains(c.status) { return c.status }
            Thread.sleep(forTimeInterval: 0.2)
        }
        return c.status
    }

    private static func lastError(_ c: NEVPNConnection) -> String? {
        guard #available(iOS 16.0, *) else { return nil }
        return try? blocking { done in
            c.fetchLastDisconnectError { error in done(error?.localizedDescription, nil) }
        }
    }

    /// Асинхронный вызов с обработчиком → синхронный. Нельзя звать с главного
    /// потока: обработчики NetworkExtension приходят именно туда.
    private static func blocking<T>(_ body: (@escaping (T, Error?) -> Void) -> Void) throws -> T {
        precondition(!Thread.isMainThread, "DetourTunnel нельзя звать с главного потока")
        let done = DispatchSemaphore(value: 0)
        var value: T?
        var failure: Error?
        body { v, e in
            value = v
            failure = e
            done.signal()
        }
        done.wait()
        if let failure { throw failure }
        return value!
    }
}

// MARK: - C-функции для Rust (client/app/src/ios.rs)

private func cString(_ s: String?) -> UnsafeMutablePointer<CChar>? {
    s.flatMap { strdup($0) }
}

@_cdecl("detour_tunnel_start")
func detour_tunnel_start(_ config: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>? {
    cString(DetourTunnel.start(config: String(cString: config)))
}

@_cdecl("detour_tunnel_stop")
func detour_tunnel_stop() {
    DetourTunnel.stop()
}

@_cdecl("detour_tunnel_running")
func detour_tunnel_running() -> Bool {
    DetourTunnel.running()
}

/// Разбор конфига — тем же libbox, что крутится в расширении.
@_cdecl("detour_tunnel_check")
func detour_tunnel_check(_ config: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>? {
    var error: NSError?
    if LibboxCheckConfig(String(cString: config), &error) { return nil }
    return cString(error?.localizedDescription ?? "конфиг не прошёл проверку")
}

@_cdecl("detour_tunnel_version")
func detour_tunnel_version() -> UnsafeMutablePointer<CChar>? {
    cString(LibboxVersion())
}

@_cdecl("detour_string_free")
func detour_string_free(_ s: UnsafeMutablePointer<CChar>?) {
    free(s)
}
