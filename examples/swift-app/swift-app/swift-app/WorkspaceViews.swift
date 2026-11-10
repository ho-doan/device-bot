import SwiftUI

struct WorkspaceShell: View {
    @Bindable var store: OrbitStore

    var body: some View {
        GeometryReader { geo in
            let wide = geo.size.width >= 800
            VStack(spacing: 0) {
                HStack {
                    Label("orbit", systemImage: "circle.circle.fill")
                        .font(.system(size: 28, weight: .heavy))
                    Spacer()
                    Button {
                        store.go("/account")
                    } label: {
                        Text("AL")
                            .font(.headline)
                            .frame(width: 40, height: 40)
                            .background(.quaternary)
                            .clipShape(Circle())
                    }
                    .accessibilityLabel("Your account")
                    .accessibilityIdentifier("account-avatar-control")
                }
                .padding(.horizontal, 16)
                .frame(height: 80)
                HStack(spacing: 0) {
                    if wide {
                        VStack {
                            navButton("Overview", selected: store.path == "/home") {
                                store.go("/home")
                            }
                            navButton("Account", selected: store.path == "/account") {
                                store.go("/account")
                            }
                            Spacer()
                            Button("Sign out", action: store.signOut)
                                .accessibilityIdentifier("rail-sign-out-control")
                                .padding(.bottom, 24)
                        }
                        .frame(width: 88)
                        Divider()
                    }
                    Group {
                        if store.path == "/account" {
                            AccountView(store: store)
                        } else {
                            HomeView(store: store)
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                if !wide {
                    HStack {
                        navButton("Overview", selected: store.path == "/home") {
                            store.go("/home")
                        }
                        navButton("Account", selected: store.path == "/account") {
                            store.go("/account")
                        }
                    }
                    .padding(.vertical, 8)
                }
            }
        }
    }

    private func navButton(_ title: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .font(.caption)
            .foregroundStyle(selected ? Color.accentColor : .secondary)
            .frame(maxWidth: .infinity, minHeight: 48)
    }
}

struct HomeView: View {
    @Bindable var store: OrbitStore
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var completed: Set<Int> = [0]
    private let tasks = [
        ("Make space for a fresh start", "5 min · A little planning"),
        ("Choose your most important task", "10 min · Set your intention"),
        ("Take a mindful break", "5 min · Recharge"),
    ]

    var body: some View {
        GeometryReader { geo in
            let stackStats = geo.size.width < 640 || typeSize >= .accessibility1
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text("YOUR WORKSPACE")
                        .font(.caption.weight(.bold))
                        .tracking(2)
                    Text("A fresh start, Alex.")
                        .font(.largeTitle.bold())
                    Text("Let’s make a little progress on what matters.")
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Find your flow.").font(.title.weight(.semibold))
                        Text("Big ideas begin with small, intentional steps.\nYour next one is right here.")
                        Button("View your profile") { store.go("/account") }
                            .buttonStyle(.borderedProminent)
                            .accessibilityIdentifier("view-profile-button")
                    }
                    .padding(28)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.quaternary)
                    .clipShape(RoundedRectangle(cornerRadius: 24))
                    Group {
                        if stackStats {
                            VStack { stats }
                        } else {
                            HStack(alignment: .top, spacing: 16) { stats }
                        }
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        Text("A little plan for today").font(.title2.weight(.semibold))
                        Text("Check things off. Enjoy the small wins.")
                        ForEach(Array(tasks.enumerated()), id: \.offset) { index, task in
                            Button {
                                if completed.contains(index) {
                                    completed.remove(index)
                                } else {
                                    completed.insert(index)
                                }
                            } label: {
                                HStack(alignment: .top) {
                                    Image(systemName: completed.contains(index) ? "checkmark.square.fill" : "square")
                                    VStack(alignment: .leading) {
                                        Text(task.0)
                                            .strikethrough(completed.contains(index))
                                            .foregroundStyle(.primary)
                                        Text(task.1).foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                }
                            }
                            .accessibilityIdentifier("task-\(index + 1)-checkbox")
                        }
                        ProgressView(value: Double(completed.count), total: 3)
                            .accessibilityLabel("\(completed.count) of 3 tasks complete")
                    }
                    .padding(24)
                    .background(.background)
                    .clipShape(RoundedRectangle(cornerRadius: 20))
                    .overlay(RoundedRectangle(cornerRadius: 20).stroke(.quaternary))
                    Text("Your pace. Your space. Your orbit.")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: 1040, alignment: .leading)
                .padding(geo.size.width >= 800 ? 40 : 24)
            }
            .accessibilityIdentifier("orbit-viewport")
        }
    }

    @ViewBuilder
    private var stats: some View {
        stat("\(completed.count)/3", "Small wins")
        stat("20 min", "Time for yourself")
        stat("Today", "A good day to begin")
    }

    private func stat(_ value: String, _ label: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(value).font(.title.weight(.semibold))
            Text(label)
        }
        .padding(24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.background)
        .clipShape(RoundedRectangle(cornerRadius: 20))
        .overlay(RoundedRectangle(cornerRadius: 20).stroke(.quaternary))
    }
}

struct AccountView: View {
    @Bindable var store: OrbitStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Your account").font(.largeTitle.bold())
                Text("A familiar face in your own little space.")
                VStack(alignment: .leading, spacing: 12) {
                    Text("AL")
                        .font(.title)
                        .frame(width: 72, height: 72)
                        .background(.quaternary)
                        .clipShape(Circle())
                    Text("Alex Lane").font(.title2)
                    Text(OrbitAuth.demoEmail).textSelection(.enabled)
                    Divider()
                    Text("Your personal workspace").font(.headline)
                    Text("You’re exploring a demo account. Changes and your sign-in session are kept only while the app is running.")
                    Button("Sign out", action: store.signOut)
                        .accessibilityIdentifier("sign-out-button")
                }
                .padding(24)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background)
                .clipShape(RoundedRectangle(cornerRadius: 20))
                .overlay(RoundedRectangle(cornerRadius: 20).stroke(.quaternary))
            }
            .frame(maxWidth: 1040, alignment: .leading)
            .padding(24)
        }
        .accessibilityIdentifier("orbit-viewport")
    }
}

struct NotFoundView: View {
    @Bindable var store: OrbitStore

    var body: some View {
        VStack(spacing: 16) {
            Text("This page is outside your orbit.")
            Button("Back to home") { store.go("/home") }
                .buttonStyle(.borderedProminent)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
