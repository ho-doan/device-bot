import Testing
@testable import swift_app

struct OrbitAuthTests {
    @Test func emailValidationAndCredentials() {
        #expect(OrbitAuth.isValidEmail("  alex@orbit.demo "))
        #expect(OrbitAuth.credentialsMatch(email: "  Alex@Orbit.demo ", password: OrbitAuth.demoPassword))
        #expect(!OrbitAuth.credentialsMatch(email: OrbitAuth.demoEmail, password: " Orbit123! "))
        #expect(!OrbitAuth.credentialsMatch(email: OrbitAuth.demoEmail, password: "orbit123!"))
    }

    @Test func destinationOnlyRestoresExactAccount() {
        #expect(OrbitAuth.destination(from: "/account") == "/account")
        #expect(OrbitAuth.destination(from: "/account?tab=profile") == "/home")
        #expect(OrbitAuth.destination(from: "/home") == "/home")
        #expect(OrbitAuth.destination(from: nil) == "/home")
    }

    @Test func routeGuards() {
        #expect(OrbitAuth.resolve(path: "/home", from: nil, signedIn: false).path == "/login")
        #expect(OrbitAuth.resolve(path: "/account", from: nil, signedIn: false).from == "/account")
        #expect(OrbitAuth.resolve(path: "/login", from: "/account", signedIn: true).path == "/account")
        #expect(OrbitAuth.resolve(path: "/missing", from: nil, signedIn: true).path == "/missing")
    }
}
