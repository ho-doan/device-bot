package com.example.android_app

import com.example.android_app.orbit.OrbitAuth
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OrbitAuthTest {
    @Test
    fun emailValidationAndCredentials() {
        assertTrue(OrbitAuth.isValidEmail("  alex@orbit.demo "))
        assertTrue(OrbitAuth.credentialsMatch("  Alex@Orbit.demo ", OrbitAuth.DEMO_PASSWORD))
        assertFalse(OrbitAuth.credentialsMatch(OrbitAuth.DEMO_EMAIL, " Orbit123! "))
        assertFalse(OrbitAuth.credentialsMatch(OrbitAuth.DEMO_EMAIL, "orbit123!"))
    }

    @Test
    fun destinationOnlyRestoresExactAccount() {
        assertEquals("/account", OrbitAuth.destinationFrom("/account"))
        assertEquals("/home", OrbitAuth.destinationFrom("/account?tab=profile"))
        assertEquals("/home", OrbitAuth.destinationFrom("/home"))
        assertEquals("/home", OrbitAuth.destinationFrom(null))
    }

    @Test
    fun routeGuards() {
        assertEquals("/login", OrbitAuth.resolve("/home", null, false).first)
        assertEquals("/account", OrbitAuth.resolve("/account", null, false).second)
        assertEquals("/account", OrbitAuth.resolve("/login", "/account", true).first)
        assertEquals("/missing", OrbitAuth.resolve("/missing", null, true).first)
    }
}
