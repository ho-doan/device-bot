<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  DEMO_EMAIL,
  DEMO_PASSWORD,
  isValidEmail,
  simulateReset
} from '../session'
import { session } from '../session-state'

defineProps<{ resetPassword: boolean }>()

const route = useRoute()
const router = useRouter()
const email = ref('')
const password = ref('')
const obscure = ref(true)
const busy = ref(false)
const sent = ref(false)
const error = ref<string | null>(null)
const emailError = ref<string | null>(null)
const passwordError = ref<string | null>(null)
const errorEl = ref<HTMLElement | null>(null)

function fromQuery(): string {
  const from = route.query.from
  return typeof from === 'string' ? `?from=${encodeURIComponent(from)}` : ''
}

async function submit(nextEmail = email.value, nextPassword = password.value): Promise<void> {
  if (busy.value) return
  const nextEmailError = isValidEmail(nextEmail) ? null : 'Enter a valid email address.'
  const nextPasswordError = route.path === '/forgot-password'
    ? null
    : nextPassword.length === 0
      ? 'Enter your password.'
      : null
  if (nextEmailError || nextPasswordError) {
    emailError.value = nextEmailError
    passwordError.value = nextPasswordError
    error.value = 'Check the highlighted fields to continue.'
    queueMicrotask(() => errorEl.value?.focus())
    return
  }
  busy.value = true
  error.value = null
  emailError.value = null
  passwordError.value = null
  if (route.path === '/forgot-password') {
    await simulateReset()
    busy.value = false
    sent.value = true
    return
  }
  const success = await session.signIn(nextEmail, nextPassword)
  if (success) return
  busy.value = false
  error.value = 'Email or password is incorrect. Try the demo account below.'
  queueMicrotask(() => errorEl.value?.focus())
}

function goLogin(): void {
  void router.push(`/login${fromQuery()}`)
}

function fillDemo(): void {
  email.value = DEMO_EMAIL
  password.value = DEMO_PASSWORD
  void submit(DEMO_EMAIL, DEMO_PASSWORD)
}
</script>

<template>
  <div class="auth-layout">
    <aside class="welcome">
      <div class="orbit-logo on-dark">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="1.8" />
          <circle cx="12" cy="12" r="3.2" fill="currentColor" />
          <circle cx="18.5" cy="7" r="1.6" fill="currentColor" />
        </svg>
        <span>orbit</span>
      </div>
      <h2>Good work starts<br />with a clear mind.</h2>
      <p>Your plans, projects, and small wins. Together in one calm space.</p>
      <div class="welcome-card">
        <div class="kicker">A LITTLE FOCUS GOES A LONG WAY</div>
        <h3>One thing at a time. One step closer.</h3>
        <div class="progress" aria-hidden="true"><span style="width: 70%" /></div>
        <p class="welcome-caption">Progress, at your own pace.</p>
      </div>
      <div class="welcome-foot">LESS FRICTION. MORE FLOW.</div>
    </aside>
    <div id="orbit-viewport" class="auth-form-pane" data-testid="orbit-viewport">
      <div class="auth-form">
        <div class="orbit-logo">
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="1.8" />
            <circle cx="12" cy="12" r="3.2" fill="currentColor" />
            <circle cx="18.5" cy="7" r="1.6" fill="currentColor" />
          </svg>
          <span>orbit</span>
        </div>
        <div v-if="sent">
          <h1 class="headline">You’re all set.</h1>
          <p class="lede">
            Reset request simulated. This demo does not send email or change your password.
          </p>
          <button
            id="back-to-sign-in-button"
            class="btn btn-filled"
            data-testid="back-to-sign-in-button"
            type="button"
            @click="goLogin"
          >
            Back to sign in
          </button>
        </div>
        <form v-else @submit.prevent="submit()">
          <div class="eyebrow">
            {{ resetPassword ? 'LET’S GET YOU BACK' : 'YOUR SPACE TO FOCUS' }}
          </div>
          <h1 class="headline">
            {{ resetPassword ? 'Forgot your password?' : 'Welcome back.' }}
          </h1>
          <p class="lede">
            {{
              resetPassword
                ? 'Enter your email to try the password reset flow.'
                : 'Make room for your best work. Sign in to your workspace.'
            }}
          </p>
          <div
            v-if="error"
            ref="errorEl"
            class="banner"
            role="alert"
            tabindex="-1"
            aria-live="polite"
          >
            {{ error }}
          </div>
          <div class="field" :class="{ error: emailError }">
            <label for="email-field">Email address</label>
            <div class="field-control">
              <input
                id="email-field"
                v-model="email"
                data-testid="email-field"
                name="email"
                type="email"
                inputmode="email"
                autocomplete="username email"
                placeholder="you@example.com"
                :disabled="busy"
              />
            </div>
            <div v-if="emailError" class="field-error">{{ emailError }}</div>
          </div>
          <template v-if="!resetPassword">
            <div class="field" :class="{ error: passwordError }">
              <label for="password-field">Password</label>
              <div class="field-control">
                <input
                  id="password-field"
                  v-model="password"
                  data-testid="password-field"
                  name="password"
                  :type="obscure ? 'password' : 'text'"
                  autocomplete="current-password"
                  :disabled="busy"
                />
                <button
                  id="password-toggle-button"
                  class="icon-btn"
                  data-testid="password-visibility-control"
                  type="button"
                  :title="obscure ? 'Show password' : 'Hide password'"
                  :aria-label="obscure ? 'Show password' : 'Hide password'"
                  @click="obscure = !obscure"
                >
                  {{ obscure ? 'Show' : 'Hide' }}
                </button>
              </div>
              <div v-if="passwordError" class="field-error">{{ passwordError }}</div>
            </div>
            <div class="row-end">
              <router-link
                id="forgot-password-button"
                class="btn-text"
                data-testid="forgot-password-button"
                :to="busy ? route.fullPath : `/forgot-password${fromQuery()}`"
                :aria-disabled="busy"
                @click="
                  (event) => {
                    if (busy) event.preventDefault()
                  }
                "
              >
                Forgot password?
              </router-link>
            </div>
          </template>
          <button
            :id="resetPassword ? 'password-reset-button' : 'sign-in-button'"
            class="btn btn-filled block"
            data-testid="submit-button"
            type="submit"
            :disabled="busy"
          >
            <span v-if="busy" class="spinner" role="status" aria-label="Please wait" />
            <template v-else>
              {{ resetPassword ? 'Try password reset' : 'Sign in' }}
              <span aria-hidden="true">→</span>
            </template>
          </button>
          <button
            v-if="resetPassword"
            id="back-to-sign-in-button"
            class="btn-text"
            data-testid="back-to-sign-in-button"
            type="button"
            :disabled="busy"
            @click="goLogin"
          >
            ← Back to sign in
          </button>
          <template v-else>
            <div style="height: 24px" />
            <div class="surface-card compact">
              <strong>Just exploring?</strong>
              <p>A ready-to-use workspace. No account needed.</p>
              <p class="credentials">{{ DEMO_EMAIL }}<br />{{ DEMO_PASSWORD }}</p>
              <button
                id="demo-button"
                class="btn btn-outlined"
                data-testid="demo-button"
                type="button"
                :disabled="busy"
                @click="fillDemo"
              >
                Try demo workspace
              </button>
            </div>
            <p class="fine-print">Demo mode · Your session ends when the app restarts.</p>
          </template>
        </form>
        <p class="tagline">A little less noise. A lot more focus.</p>
      </div>
    </div>
  </div>
</template>
