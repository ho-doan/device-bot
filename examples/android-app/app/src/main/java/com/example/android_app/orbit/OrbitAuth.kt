package com.example.android_app.orbit

object OrbitAuth {
    const val DEMO_EMAIL = "alex@orbit.demo"
    const val DEMO_PASSWORD = "Orbit123!"
    const val DELAY_MS = 700L
    val TASKS = listOf(
        "Make space for a fresh start" to "5 min · A little planning",
        "Choose your most important task" to "10 min · Set your intention",
        "Take a mindful break" to "5 min · Recharge",
    )
    val knownPaths = setOf("/", "/login", "/forgot-password", "/home", "/account")
    private val emailPattern = Regex("""^[^\s@]+@[^\s@]+\.[^\s@]+$""")

    fun isValidEmail(value: String): Boolean = emailPattern.matches(value.trim())

    fun credentialsMatch(email: String, password: String): Boolean =
        email.trim().lowercase() == DEMO_EMAIL && password == DEMO_PASSWORD

    fun destinationFrom(from: String?): String = if (from == "/account") "/account" else "/home"

    fun resolve(path: String, from: String?, signedIn: Boolean): Pair<String, String?> {
        if (path == "/") return resolve("/home", from, signedIn)
        val isAuth = path == "/login" || path == "/forgot-password"
        val isProtected = path == "/home" || path == "/account"
        if (!signedIn && isProtected) return "/login" to path
        if (signedIn && isAuth) return destinationFrom(from) to null
        return path to from
    }
}
