import SwiftUI

struct OrbitRootView: View {
    @State private var store: OrbitStore
    @Environment(\.colorScheme) private var colorScheme

    init(store: OrbitStore = OrbitStore()) {
        _store = State(initialValue: store)
    }

    var body: some View {
        Group {
            if !OrbitAuth.knownPaths.contains(store.path) {
                NotFoundView(store: store)
            } else if store.path == "/login" || store.path == "/forgot-password" {
                AuthView(store: store, resetPassword: store.path == "/forgot-password")
            } else {
                WorkspaceShell(store: store)
            }
        }
        .tint(colorScheme == .dark ? Color(red: 0.50, green: 0.87, blue: 0.82) : Color(red: 0.03, green: 0.42, blue: 0.38))
    }
}
