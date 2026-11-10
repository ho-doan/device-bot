import SwiftUI

struct AuthView: View {
    @Bindable var store: OrbitStore
    var resetPassword: Bool
    @Environment(\.colorScheme) private var scheme
    @State private var email = ""
    @State private var password = ""
    @State private var obscure = true
    @State private var busy = false
    @State private var sent = false
    @State private var error: String?
    @State private var emailError: String?
    @State private var passwordError: String?

    var body: some View {
        GeometryReader { geo in
            let wide = geo.size.width >= 960
            HStack(spacing: 0) {
                if wide {
                    welcomePanel
                }
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        logo
                        if sent {
                            sentContent
                        } else {
                            formContent
                        }
                        Text("A little less noise. A lot more focus.")
                            .foregroundStyle(.secondary)
                            .padding(.top, 16)
                    }
                    .frame(maxWidth: 420, alignment: .leading)
                    .padding(.horizontal, wide ? 56 : 24)
                    .padding(.vertical, 32)
                    .frame(maxWidth: .infinity)
                }
                .accessibilityIdentifier("orbit-viewport")
            }
        }
        .background(scheme == .dark ? Color(red: 0.06, green: 0.12, blue: 0.13) : Color(red: 0.97, green: 0.98, blue: 0.97))
    }

    private var primary: Color {
        scheme == .dark ? Color(red: 0.50, green: 0.87, blue: 0.82) : Color(red: 0.03, green: 0.42, blue: 0.38)
    }

    private var logo: some View {
        Label("orbit", systemImage: "circle.circle.fill")
            .font(.system(size: 30, weight: .heavy))
            .foregroundStyle(primary)
            .labelStyle(.titleAndIcon)
    }

    private var formContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(resetPassword ? "LET’S GET YOU BACK" : "YOUR SPACE TO FOCUS")
                .font(.caption.weight(.bold))
                .tracking(2)
                .foregroundStyle(primary)
            Text(resetPassword ? "Forgot your password?" : "Welcome back.")
                .font(.largeTitle.bold())
            Text(
                resetPassword
                    ? "Enter your email to try the password reset flow."
                    : "Make room for your best work. Sign in to your workspace."
            )
            .foregroundStyle(.secondary)
            if let error {
                Text(error)
                    .padding(16)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(scheme == .dark ? Color(red: 0.58, green: 0, blue: 0.04) : Color(red: 1, green: 0.85, blue: 0.84))
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .accessibilityAddTraits(.updatesFrequently)
            }
            Text("Email address")
            TextField("you@example.com", text: $email)
                .textContentType(.username)
                .textFieldStyle(.roundedBorder)
                .disabled(busy)
                .accessibilityIdentifier("email-field")
            if let emailError {
                Text(emailError).foregroundStyle(.red)
            }
            if !resetPassword {
                Text("Password")
                HStack {
                    Group {
                        if obscure {
                            SecureField("Password", text: $password)
                        } else {
                            TextField("Password", text: $password)
                        }
                    }
                    .textContentType(.password)
                    .disabled(busy)
                    .accessibilityIdentifier("password-field")
                    Button(obscure ? "Show password" : "Hide password") {
                        obscure.toggle()
                    }
                    .disabled(false)
                    .accessibilityIdentifier("password-visibility-control")
                }
                if let passwordError {
                    Text(passwordError).foregroundStyle(.red)
                }
                HStack {
                    Spacer()
                    Button("Forgot password?") {
                        store.go("/forgot-password", from: store.from)
                    }
                    .disabled(busy)
                    .accessibilityIdentifier("forgot-password-button")
                }
            }
            Button {
                Task { await submit() }
            } label: {
                if busy {
                    ProgressView()
                        .accessibilityLabel("Please wait")
                        .frame(maxWidth: .infinity, minHeight: 24)
                } else {
                    Text(resetPassword ? "Try password reset" : "Sign in")
                        .frame(maxWidth: .infinity)
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(busy)
            .accessibilityIdentifier("submit-button")
            if resetPassword {
                Button("Back to sign in") {
                    store.go("/login", from: store.from)
                }
                .disabled(busy)
                .accessibilityIdentifier("back-to-sign-in-button")
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Just exploring?").font(.headline)
                    Text("A ready-to-use workspace. No account needed.")
                    Text("\(OrbitAuth.demoEmail)\n\(OrbitAuth.demoPassword)")
                        .textSelection(.enabled)
                    Button("Try demo workspace") {
                        email = OrbitAuth.demoEmail
                        password = OrbitAuth.demoPassword
                        Task { await submit(nextEmail: OrbitAuth.demoEmail, nextPassword: OrbitAuth.demoPassword) }
                    }
                    .disabled(busy)
                    .accessibilityIdentifier("demo-button")
                }
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.background)
                .clipShape(RoundedRectangle(cornerRadius: 20))
                .overlay(RoundedRectangle(cornerRadius: 20).stroke(.quaternary))
                Text("Demo mode · Your session ends when the app restarts.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var sentContent: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("You’re all set.").font(.largeTitle.bold())
            Text("Reset request simulated. This demo does not send email or change your password.")
            Button("Back to sign in") {
                store.go("/login", from: store.from)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("back-to-sign-in-button")
        }
        .padding(.top, 48)
    }

    private var welcomePanel: some View {
        VStack(alignment: .leading, spacing: 24) {
            Label("orbit", systemImage: "circle.circle.fill")
                .font(.system(size: 30, weight: .heavy))
                .foregroundStyle(.white)
            Text("Good work starts\nwith a clear mind.")
                .font(.system(size: 48, weight: .semibold))
                .foregroundStyle(.white)
            Text("Your plans, projects, and small wins.\nTogether in one calm space.")
                .foregroundStyle(Color(red: 0.77, green: 0.89, blue: 0.84))
            Spacer()
        }
        .padding(48)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color(red: 0.07, green: 0.24, blue: 0.22))
        .clipShape(RoundedRectangle(cornerRadius: 28))
        .padding(16)
    }

    @MainActor
    private func submit(nextEmail: String? = nil, nextPassword: String? = nil) async {
        if busy { return }
        let emailValue = nextEmail ?? email
        let passwordValue = nextPassword ?? password
        let nextEmailError = OrbitAuth.isValidEmail(emailValue) ? nil : "Enter a valid email address."
        let nextPasswordError = resetPassword
            ? nil
            : (passwordValue.isEmpty ? "Enter your password." : nil)
        if nextEmailError != nil || nextPasswordError != nil {
            emailError = nextEmailError
            passwordError = nextPasswordError
            error = "Check the highlighted fields to continue."
            return
        }
        busy = true
        error = nil
        emailError = nil
        passwordError = nil
        if resetPassword {
            try? await Task.sleep(nanoseconds: OrbitAuth.delayNs)
            busy = false
            sent = true
            return
        }
        let success = await store.signIn(email: emailValue, password: passwordValue)
        if !success {
            busy = false
            error = "Email or password is incorrect. Try the demo account below."
        }
    }
}

