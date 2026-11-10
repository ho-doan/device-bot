<script setup lang="ts">
import { DEMO_EMAIL } from '../session'
import { session } from '../session-state'
</script>

<template>
  <main id="orbit-viewport" class="workspace" data-testid="orbit-viewport">
    <div class="workspace-inner">
      <h1 class="headline">Your account</h1>
      <p class="lede">A familiar face in your own little space.</p>
      <div class="surface-card">
        <div class="avatar lg">AL</div>
        <h2>Alex Lane</h2>
        <p class="credentials">{{ DEMO_EMAIL }}</p>
        <hr />
        <h3>Your personal workspace</h3>
        <p>
          You’re exploring a demo account. Changes and your sign-in session are kept only while the
          app is running.
        </p>
        <button
          id="sign-out-button"
          class="btn btn-outlined"
          data-testid="sign-out-button"
          type="button"
          @click="session.signOut()"
        >
          Sign out
        </button>
      </div>
    </div>
  </main>
</template>
