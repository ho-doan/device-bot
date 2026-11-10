package com.example.android_app.orbit

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Logout
import androidx.compose.material.icons.outlined.GridView
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Visibility
import androidx.compose.material.icons.outlined.VisibilityOff
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.example.android_app.ui.theme.AndroidappTheme

@Composable
fun OrbitApp(vm: OrbitViewModel = viewModel()) {
    AndroidappTheme {
        when {
            vm.path !in OrbitAuth.knownPaths -> NotFoundScreen(onHome = { vm.go("/home") })
            vm.path == "/login" || vm.path == "/forgot-password" ->
                AuthScreen(vm, resetPassword = vm.path == "/forgot-password")
            else -> WorkspaceShell(vm)
        }
    }
}

@Composable
private fun AuthScreen(vm: OrbitViewModel, resetPassword: Boolean) {
    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var obscure by remember { mutableStateOf(true) }
    var busy by remember { mutableStateOf(false) }
    var sent by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var emailError by remember { mutableStateOf<String?>(null) }
    var passwordError by remember { mutableStateOf<String?>(null) }

    fun submit(nextEmail: String = email, nextPassword: String = password) {
        if (busy) return
        val nextEmailError =
            if (OrbitAuth.isValidEmail(nextEmail)) null else "Enter a valid email address."
        val nextPasswordError =
            if (resetPassword) null else if (nextPassword.isEmpty()) "Enter your password." else null
        if (nextEmailError != null || nextPasswordError != null) {
            emailError = nextEmailError
            passwordError = nextPasswordError
            error = "Check the highlighted fields to continue."
            return
        }
        busy = true
        error = null
        emailError = null
        passwordError = null
        if (resetPassword) {
            vm.simulateReset {
                busy = false
                sent = true
            }
            return
        }
        vm.signIn(nextEmail, nextPassword) { success ->
            if (!success) {
                busy = false
                error = "Email or password is incorrect. Try the demo account below."
            }
        }
    }

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= 960.dp
        Row(Modifier.fillMaxSize()) {
            if (wide) {
                Surface(
                    modifier = Modifier.weight(1f).padding(16.dp),
                    color = androidx.compose.ui.graphics.Color(0xFF123D37),
                    shape = MaterialTheme.shapes.extraLarge,
                ) {
                    Column(Modifier.padding(48.dp)) {
                        Text("orbit", color = androidx.compose.ui.graphics.Color.White, fontWeight = FontWeight.ExtraBold)
                        Spacer(Modifier.height(48.dp))
                        Text("Good work starts\nwith a clear mind.", color = androidx.compose.ui.graphics.Color.White, style = MaterialTheme.typography.headlineLarge)
                    }
                }
            }
            Column(
                Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = if (wide) 56.dp else 24.dp, vertical = 32.dp)
                    .testTag("orbit-viewport"),
            ) {
                Text("orbit", style = MaterialTheme.typography.headlineMedium, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.ExtraBold)
                if (sent) {
                    Text("You’re all set.", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
                    Text("Reset request simulated. This demo does not send email or change your password.")
                    Button(onClick = { vm.go("/login", vm.from) }, modifier = Modifier.testTag("back-to-sign-in-button")) {
                        Text("Back to sign in")
                    }
                } else {
                    Text(if (resetPassword) "LET’S GET YOU BACK" else "YOUR SPACE TO FOCUS", color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
                    Text(if (resetPassword) "Forgot your password?" else "Welcome back.", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
                    Text(
                        if (resetPassword) "Enter your email to try the password reset flow."
                        else "Make room for your best work. Sign in to your workspace.",
                    )
                    error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(vertical = 12.dp)) }
                    OutlinedTextField(
                        value = email,
                        onValueChange = { email = it },
                        enabled = !busy,
                        label = { Text("Email address") },
                        placeholder = { Text("you@example.com") },
                        isError = emailError != null,
                        supportingText = emailError?.let { { Text(it) } },
                        modifier = Modifier.fillMaxWidth().testTag("email-field"),
                    )
                    if (!resetPassword) {
                        OutlinedTextField(
                            value = password,
                            onValueChange = { password = it },
                            enabled = !busy,
                            label = { Text("Password") },
                            isError = passwordError != null,
                            supportingText = passwordError?.let { { Text(it) } },
                            visualTransformation = if (obscure) PasswordVisualTransformation() else VisualTransformation.None,
                            trailingIcon = {
                                IconButton(onClick = { obscure = !obscure }, modifier = Modifier.testTag("password-visibility-control")) {
                                    Icon(
                                        if (obscure) Icons.Outlined.Visibility else Icons.Outlined.VisibilityOff,
                                        contentDescription = if (obscure) "Show password" else "Hide password",
                                    )
                                }
                            },
                            modifier = Modifier.fillMaxWidth().testTag("password-field"),
                        )
                        TextButton(
                            onClick = { vm.go("/forgot-password", vm.from) },
                            enabled = !busy,
                            modifier = Modifier.align(Alignment.End).testTag("forgot-password-button"),
                        ) { Text("Forgot password?") }
                    }
                    Button(
                        onClick = { submit() },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().height(56.dp).testTag("submit-button"),
                    ) {
                        if (busy) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                        else Text(if (resetPassword) "Try password reset" else "Sign in")
                    }
                    if (resetPassword) {
                        TextButton(onClick = { vm.go("/login", vm.from) }, enabled = !busy, modifier = Modifier.testTag("back-to-sign-in-button")) {
                            Text("Back to sign in")
                        }
                    } else {
                        Spacer(Modifier.height(24.dp))
                        Surface(tonalElevation = 1.dp, shape = MaterialTheme.shapes.large) {
                            Column(Modifier.padding(20.dp)) {
                                Text("Just exploring?", fontWeight = FontWeight.SemiBold)
                                Text("A ready-to-use workspace. No account needed.")
                                Text("${OrbitAuth.DEMO_EMAIL}\n${OrbitAuth.DEMO_PASSWORD}")
                                OutlinedButton(
                                    onClick = {
                                        email = OrbitAuth.DEMO_EMAIL
                                        password = OrbitAuth.DEMO_PASSWORD
                                        submit(OrbitAuth.DEMO_EMAIL, OrbitAuth.DEMO_PASSWORD)
                                    },
                                    enabled = !busy,
                                    modifier = Modifier.testTag("demo-button"),
                                ) { Text("Try demo workspace") }
                            }
                        }
                        Text("Demo mode · Your session ends when the app restarts.", style = MaterialTheme.typography.bodySmall)
                    }
                }
                Text("A little less noise. A lot more focus.", color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 32.dp))
            }
        }
    }
}

@Composable
private fun WorkspaceShell(vm: OrbitViewModel) {
    val account = vm.path == "/account"
    BoxWithConstraints {
        val wide = maxWidth >= 800.dp
        Scaffold(
            topBar = {
                Row(
                    Modifier.fillMaxWidth().height(80.dp).padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("orbit", style = MaterialTheme.typography.headlineSmall, color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.ExtraBold)
                    IconButton(onClick = { vm.go("/account") }, modifier = Modifier.testTag("account-avatar-control")) {
                        Surface(Modifier.size(40.dp).clip(CircleShape), color = MaterialTheme.colorScheme.primaryContainer) {
                            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text("AL") }
                        }
                    }
                }
            },
            bottomBar = {
                if (!wide) {
                    NavigationBar {
                        NavigationBarItem(
                            selected = !account,
                            onClick = { vm.go("/home") },
                            icon = { Icon(Icons.Outlined.GridView, null) },
                            label = { Text("Overview") },
                        )
                        NavigationBarItem(
                            selected = account,
                            onClick = { vm.go("/account") },
                            icon = { Icon(Icons.Outlined.Person, null) },
                            label = { Text("Account") },
                        )
                    }
                }
            },
        ) { padding ->
            Row(Modifier.fillMaxSize().padding(padding)) {
                if (wide) {
                    NavigationRail {
                        NavigationRailItem(
                            selected = !account,
                            onClick = { vm.go("/home") },
                            icon = { Icon(Icons.Outlined.GridView, null) },
                            label = { Text("Overview") },
                        )
                        NavigationRailItem(
                            selected = account,
                            onClick = { vm.go("/account") },
                            icon = { Icon(Icons.Outlined.Person, null) },
                            label = { Text("Account") },
                        )
                        Spacer(Modifier.weight(1f))
                        IconButton(onClick = vm::signOut, modifier = Modifier.testTag("rail-sign-out-control")) {
                            Icon(Icons.AutoMirrored.Outlined.Logout, contentDescription = "Sign out")
                        }
                    }
                }
                if (account) AccountScreen(vm) else HomeScreen(vm)
            }
        }
    }
}

@Composable
private fun HomeScreen(vm: OrbitViewModel) {
    var completed by remember { mutableStateOf(setOf(0)) }
    val fontScale = LocalDensity.current.fontScale
    BoxWithConstraints(Modifier.fillMaxSize().testTag("orbit-viewport")) {
        val stack = maxWidth < 640.dp || fontScale * 16 > 24
        val pad = if (maxWidth + 88.dp >= 800.dp) 40.dp else 24.dp
        Column(Modifier.verticalScroll(rememberScrollState()).padding(pad)) {
            Text("YOUR WORKSPACE", color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.Bold)
            Text("A fresh start, Alex.", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
            Text("Let’s make a little progress on what matters.", color = MaterialTheme.colorScheme.onSurfaceVariant)
            Surface(Modifier.fillMaxWidth().padding(vertical = 24.dp), color = MaterialTheme.colorScheme.primaryContainer, shape = MaterialTheme.shapes.extraLarge) {
                Column(Modifier.padding(28.dp)) {
                    Text("Find your flow.", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                    Text("Big ideas begin with small, intentional steps.\nYour next one is right here.")
                    Button(onClick = { vm.go("/account") }, modifier = Modifier.testTag("view-profile-button")) { Text("View your profile") }
                }
            }
            if (stack) {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Stat("${completed.size}/3", "Small wins")
                    Stat("20 min", "Time for yourself")
                    Stat("Today", "A good day to begin")
                }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    BoxWithConstraints(Modifier.weight(1f)) { Stat("${completed.size}/3", "Small wins") }
                    BoxWithConstraints(Modifier.weight(1f)) { Stat("20 min", "Time for yourself") }
                    BoxWithConstraints(Modifier.weight(1f)) { Stat("Today", "A good day to begin") }
                }
            }
            Spacer(Modifier.height(24.dp))
            Surface(Modifier.fillMaxWidth(), tonalElevation = 1.dp, shape = MaterialTheme.shapes.large) {
                Column(Modifier.padding(24.dp)) {
                    Text("A little plan for today", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                    Text("Check things off. Enjoy the small wins.")
                    OrbitAuth.TASKS.forEachIndexed { index, task ->
                        val done = index in completed
                        Row(
                            Modifier.testTag("task-${index + 1}-checkbox"),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(
                                checked = done,
                                onCheckedChange = {
                                    completed = if (it) completed + index else completed - index
                                },
                            )
                            Column {
                                Text(task.first, textDecoration = if (done) TextDecoration.LineThrough else null)
                                Text(task.second, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                    LinearProgressIndicator(
                        progress = { completed.size / 3f },
                        modifier = Modifier.fillMaxWidth().semantics { contentDescription = "${completed.size} of 3 tasks complete" },
                    )
                }
            }
            Text("Your pace. Your space. Your orbit.", color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 24.dp))
        }
    }
}

@Composable
private fun Stat(value: String, label: String) {
    Surface(Modifier.fillMaxWidth(), tonalElevation = 1.dp, shape = MaterialTheme.shapes.large) {
        Column(Modifier.padding(24.dp)) {
            Text(value, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
            Text(label)
        }
    }
}

@Composable
private fun AccountScreen(vm: OrbitViewModel) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp)
            .testTag("orbit-viewport"),
    ) {
        Text("Your account", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.Bold)
        Text("A familiar face in your own little space.")
        Spacer(Modifier.height(24.dp))
        Surface(Modifier.fillMaxWidth(), tonalElevation = 1.dp, shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(24.dp)) {
                Surface(Modifier.size(72.dp).clip(CircleShape), color = MaterialTheme.colorScheme.primaryContainer) {
                    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text("AL") }
                }
                Text("Alex Lane", style = MaterialTheme.typography.headlineSmall)
                Text(OrbitAuth.DEMO_EMAIL)
                Text("Your personal workspace", fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(top = 16.dp))
                Text("You’re exploring a demo account. Changes and your sign-in session are kept only while the app is running.")
                OutlinedButton(onClick = vm::signOut, modifier = Modifier.testTag("sign-out-button")) { Text("Sign out") }
            }
        }
    }
}

@Composable
private fun NotFoundScreen(onHome: () -> Unit) {
    Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
        Text("This page is outside your orbit.")
        Spacer(Modifier.height(16.dp))
        Button(onClick = onHome) { Text("Back to home") }
    }
}
