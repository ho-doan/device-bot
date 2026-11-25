import Foundation
import Network

struct Probe {
    let target: String
    let proxy: String
    let control: String
    let healthy: String
    let token: String
    let style: String

    init(_ env: [String: String]) {
        target = env["SPIKE_TARGET"] ?? "127.0.0.1:8080"
        proxy = env["SPIKE_PROXY"] ?? "127.0.0.1:18765"
        control = env["SPIKE_CONTROL"] ?? "127.0.0.1:18766"
        healthy = env["SPIKE_HEALTHY"] ?? target
        token = env["SPIKE_TOKEN"] ?? "spike"
        style = env["SPIKE_STYLE"] ?? "connect"
    }

    func session(_ style: String) -> URLSession {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 0.65
        config.timeoutIntervalForResource = 3
        config.urlCache = nil
        let pair = proxy.split(separator: ":")
        let endpoint = NWEndpoint.hostPort(
            host: NWEndpoint.Host(String(pair[0])),
            port: NWEndpoint.Port(rawValue: UInt16(pair[1])!)!
        )
        if style == "connect" {
            config.proxyConfigurations = [.init(httpCONNECTProxy: endpoint, tlsOptions: nil)]
        } else if style == "legacy" {
            config.connectionProxyDictionary = [
                "HTTPEnable": 1, "HTTPProxy": String(pair[0]), "HTTPPort": Int(pair[1])!,
                "ExcludeSimpleHostnames": false, "ExceptionsList": []
            ]
        } else {
            config.connectionProxyDictionary = [:]
        }
        return URLSession(configuration: config)
    }

    func http(_ authority: String, path: String, using session: URLSession) async -> String {
        do {
            let (_, response) = try await session.data(from: URL(string: "http://\(authority)\(path)")!)
            return String((response as? HTTPURLResponse)?.statusCode ?? 0)
        } catch {
            return "ERR:\((error as NSError).code)"
        }
    }

    func ws(_ session: URLSession, expectCut: Bool = false) async -> String {
        let ws = session.webSocketTask(with: URL(string: "ws://\(target)/socket")!)
        ws.resume()
        defer { ws.cancel(with: .goingAway, reason: nil) }
        return await withTaskGroup(of: String.self) { group in
            group.addTask {
                do {
                    guard case .string("healthy") = try await ws.receive() else { return "unexpected-frame" }
                } catch { return "ERR:\((error as NSError).code)" }
                guard expectCut else { return "healthy" }
                do {
                    _ = try await ws.receive()
                    return "unexpected-second-frame"
                } catch { return "cut:\((error as NSError).code)" }
            }
            group.addTask {
                do { try await Task.sleep(for: .seconds(2)) } catch { return "cancelled" }
                return "watchdog"
            }
            let first = await group.next() ?? "empty"
            if first == "watchdog" { ws.cancel() }
            group.cancelAll()
            return first
        }
    }

    func run() async -> String {
        let direct = session("direct")
        let subject = session(style)
        defer { direct.invalidateAndCancel(); subject.invalidateAndCancel() }
        var rows = ["target=\(target) style=\(style)"]
        func mode(_ value: String) async -> Bool {
            await http(control, path: "/\(token)/\(value)", using: direct) == "200"
        }
        guard await mode("pass") else { return "CONTROL_UNREACHABLE\n" + rows[0] }
        rows.append("direct_initial=\(await http(healthy, path: "/items", using: direct))")
        rows.append("pass=\(await http(target, path: "/items", using: subject))")
        for fault in ["503", "delay", "drop"] {
            guard await mode(fault) else { return rows.joined(separator: "\n") + "\nCONTROL_LOST" }
            let start = Date()
            let answer = await http(target, path: "/items", using: subject)
            rows.append("\(fault)=\(answer) ms=\(Int(Date().timeIntervalSince(start) * 1000))")
            if fault == "503" {
                rows.append("direct_during_503=\(await http(healthy, path: "/items", using: direct))")
            }
        }
        for fault in ["pass", "ws-reject", "ws-drop", "pass"] {
            guard await mode(fault) else { return rows.joined(separator: "\n") + "\nCONTROL_LOST" }
            rows.append("ws_\(fault)=\(await ws(subject, expectCut: fault == "ws-drop"))")
        }
        rows.append("recovery=\(await http(target, path: "/items", using: subject))")
        return rows.joined(separator: "\n")
    }
}

@main struct Main {
    static func main() async {
        print(await Probe(ProcessInfo.processInfo.environment).run())
    }
}
