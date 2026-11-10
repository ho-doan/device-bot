package com.example.android_app.orbit

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class OrbitViewModel : ViewModel() {
    var signedIn by mutableStateOf(false)
        private set
    var path by mutableStateOf("/login")
        private set
    var from by mutableStateOf<String?>("/home")
        private set

    init {
        go("/")
    }

    fun go(target: String, newFrom: String? = from) {
        val (nextPath, nextFrom) = OrbitAuth.resolve(target, newFrom, signedIn)
        path = nextPath
        from = nextFrom
    }

    fun signIn(email: String, password: String, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            delay(OrbitAuth.DELAY_MS)
            val success = OrbitAuth.credentialsMatch(email, password)
            if (success) {
                signedIn = true
                go(path, from)
            }
            onResult(success)
        }
    }

    fun simulateReset(onDone: () -> Unit) {
        viewModelScope.launch {
            delay(OrbitAuth.DELAY_MS)
            onDone()
        }
    }

    fun signOut() {
        signedIn = false
        go(path, from)
    }
}
