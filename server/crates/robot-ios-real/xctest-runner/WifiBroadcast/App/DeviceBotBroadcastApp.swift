import Network
import ReplayKit
import SwiftUI

@main
struct DeviceBotBroadcastApp: App {
    var body: some Scene { WindowGroup { BroadcastView() } }
}

struct BroadcastPicker: UIViewRepresentable {
    func makeUIView(context: Context) -> RPSystemBroadcastPickerView {
        let picker = RPSystemBroadcastPickerView(frame: CGRect(x: 0, y: 0, width: 60, height: 60))
        picker.preferredExtension = Bundle.main.object(forInfoDictionaryKey: "WifiBroadcastExtension") as? String
        picker.showsMicrophoneButton = false
        picker.accessibilityLabel = "Share screen"
        picker.accessibilityIdentifier = "wifi-broadcast-picker"
        return picker
    }
    func updateUIView(_ view: RPSystemBroadcastPickerView, context: Context) {}
}

struct BroadcastView: View {
    @State private var configuration: BroadcastConfiguration?
    @State private var status = "Waiting for your Mac"
    @State private var connected = false
    @State private var browser: NWBrowser?

    var body: some View {
        NavigationView {
            Form {
                Section {
                    Label(status, systemImage: connected ? "wifi" : "wifi.slash")
                        .accessibilityIdentifier("wifi-broadcast-status")
                    if let host = configuration?.endpoint.host {
                        HStack { Text("Mac"); Spacer(); Text(host).foregroundStyle(.secondary) }
                    }
                } header: { Text("Connection") }

                Section {
                    if connected {
                        HStack(alignment: .center, spacing: 16) {
                            BroadcastPicker().frame(width: 60, height: 60)
                            Text("Tap the record button, then choose Start Broadcast.")
                        }
                    } else {
                        Label("Open Live on your Mac and allow Local Network access on this iPhone.", systemImage: "info.circle")
                    }
                } header: { Text("Share your screen") }
                  footer: { Text("After the countdown, switch to the app you want to control. Only screen video is sent; audio is not streamed.") }

                Section {
                    Text("Close Live on your Mac or use the screen recording control on this iPhone to stop sharing.")
                } header: { Text("Stop sharing") }
            }
            .navigationTitle("Device Bot Broadcast")
            .task { await connect() }
        }
        .navigationViewStyle(.stack)
    }

    @MainActor
    private func connect() async {
        do {
            try BroadcastConfiguration.saveLaunchConfiguration()
            configuration = try BroadcastConfiguration.load(requireFresh: false)
        } catch {
            status = error.localizedDescription
            return
        }
        // Browsing triggers the platform's Local Network consent sheet. The
        // stream destination itself is paired by the Mac, not discovered here.
        let discovery = NWBrowser(for: .bonjour(type: "_devicebot-video._tcp", domain: nil), using: .tcp)
        browser = discovery
        discovery.start(queue: .main)
        defer { discovery.cancel(); browser = nil }
        while !Task.isCancelled {
            guard let configuration else { return }
            do {
                let (_, response) = try await URLSession.shared.data(for: configuration.request(method: "GET"))
                let http = response as? HTTPURLResponse
                connected = http?.statusCode == 204
                status = connected ? (http?.value(forHTTPHeaderField: "X-Broadcast-State") == "sharing" ? "Sharing screen" : "Ready to share") : "Pairing ended. Reopen Live on your Mac."
            } catch {
                connected = false
                status = "Cannot reach your Mac. Check Wi-Fi and Local Network access."
            }
            do { try await Task.sleep(nanoseconds: 1_000_000_000) } catch { return }
        }
    }
}

#Preview("Light") { BroadcastView().preferredColorScheme(.light) }
#Preview("Dark") { BroadcastView().preferredColorScheme(.dark) }
