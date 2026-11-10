import Foundation
import Observation

enum OrbitAuth {
    static let demoEmail = "alex@orbit.demo"
    static let demoPassword = "Orbit123!"
    static let delayNs: UInt64 = 700_000_000
    static let knownPaths: Set<String> = ["/", "/login", "/forgot-password", "/home", "/account"]

    static func isValidEmail(_ value: String) -> Bool {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.range(of: #"^[^\s@]+@[^\s@]+\.[^\s@]+$"#, options: .regularExpression) != nil
    }

    static func credentialsMatch(email: String, password: String) -> Bool {
        email.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == demoEmail
            && password == demoPassword
    }

    static func destination(from: String?) -> String {
        from == "/account" ? "/account" : "/home"
    }

    static func resolve(path: String, from: String?, signedIn: Bool) -> (path: String, from: String?) {
        if path == "/" {
            return resolve(path: "/home", from: from, signedIn: signedIn)
        }
        let isAuth = path == "/login" || path == "/forgot-password"
        let isProtected = path == "/home" || path == "/account"
        if !signedIn && isProtected {
            return ("/login", path)
        }
        if signedIn && isAuth {
            return (destination(from: from), nil)
        }
        return (path, from)
    }
}

@Observable
final class OrbitStore {
    var signedIn = false
    var path = "/login"
    var from: String? = "/home"

    init(initialPath: String = "/") {
        go(initialPath)
    }

    func go(_ target: String, from newFrom: String? = nil) {
        let resolved = OrbitAuth.resolve(
            path: target,
            from: newFrom ?? from,
            signedIn: signedIn
        )
        path = resolved.path
        from = resolved.from
    }

    func signIn(email: String, password: String) async -> Bool {
        try? await Task.sleep(nanoseconds: OrbitAuth.delayNs)
        guard OrbitAuth.credentialsMatch(email: email, password: password) else {
            return false
        }
        signedIn = true
        go(path, from: from)
        return true
    }

    func signOut() {
        signedIn = false
        go(path, from: from)
    }
}
