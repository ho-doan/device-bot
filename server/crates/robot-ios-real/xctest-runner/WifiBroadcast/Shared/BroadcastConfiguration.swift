import Foundation
import Security

struct BroadcastConfiguration: Codable {
    let endpoint: URL
    let token: String
    let expiresAt: TimeInterval

    // Both signed targets share only this credential item. No App Group
    // container or developer-portal App Group registration is necessary.
    private static var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: "devicebot-wifi-live",
         kSecAttrAccount as String: "configuration",
         kSecAttrAccessGroup as String: Bundle.main.object(forInfoDictionaryKey: "WifiKeychainGroup") as? String ?? ""]
    }

    static func load(requireFresh: Bool = true) throws -> BroadcastConfiguration {
        var lookup = query
        lookup[kSecReturnData as String] = true
        lookup[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let result = SecItemCopyMatching(lookup as CFDictionary, &item)
        guard result == errSecSuccess, let data = item as? Data else {
            throw BroadcastError.message("Open Live on your Mac to pair this iPhone (Keychain \(result)).")
        }
        let configuration = try JSONDecoder().decode(Self.self, from: data)
        guard configuration.token.count == 64, configuration.endpoint.scheme == "http",
              configuration.endpoint.host != nil,
              configuration.endpoint.path.hasPrefix("/wifi-video/"),
              !requireFresh || configuration.expiresAt > Date().timeIntervalSince1970 else {
            throw BroadcastError.message("Pairing expired. Open Live on your Mac to pair again.")
        }
        return configuration
    }

    static func saveLaunchConfiguration() throws {
        guard let raw = ProcessInfo.processInfo.environment["DEVICE_BOT_WIFI_CONFIG"] else { return }
        let data = Data(raw.utf8)
        _ = try JSONDecoder().decode(Self.self, from: data)
        let values: [String: Any] = [kSecValueData as String: data, kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly]
        var result = SecItemUpdate(query as CFDictionary, values as CFDictionary)
        if result == errSecItemNotFound {
            result = SecItemAdd(query.merging(values) { _, new in new } as CFDictionary, nil)
        }
        guard result == errSecSuccess else { throw BroadcastError.message("Cannot save pairing credentials (Keychain \(result)).") }
    }

    func request(method: String) -> URLRequest {
        var request = URLRequest(url: endpoint)
        request.httpMethod = method
        request.timeoutInterval = 6
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        return request
    }
}

enum BroadcastError: LocalizedError {
    case message(String)
    var errorDescription: String? {
        switch self { case .message(let message): return message }
    }
}
