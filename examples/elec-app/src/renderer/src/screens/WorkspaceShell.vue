<script setup lang="ts">
import { useRouter } from 'vue-router'
import { session } from '../session-state'

const router = useRouter()
</script>

<template>
  <div class="shell">
    <header class="topbar">
      <div class="orbit-logo">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" stroke-width="1.8" />
          <circle cx="12" cy="12" r="3.2" fill="currentColor" />
          <circle cx="18.5" cy="7" r="1.6" fill="currentColor" />
        </svg>
        <span>orbit</span>
      </div>
      <button
        id="account-avatar-button"
        class="icon-btn"
        data-testid="account-avatar-control"
        type="button"
        title="Your account"
        aria-label="Your account"
        @click="router.push({ name: 'account' })"
      >
        <span class="avatar">AL</span>
      </button>
    </header>
    <div class="shell-body">
      <nav class="rail" aria-label="Workspace">
        <router-link class="nav-item" active-class="active" :to="{ name: 'home' }">
          Overview
        </router-link>
        <router-link class="nav-item" active-class="active" :to="{ name: 'account' }">
          Account
        </router-link>
        <div class="rail-spacer" />
        <button
          id="rail-sign-out-button"
          class="icon-btn"
          data-testid="rail-sign-out-control"
          type="button"
          title="Sign out"
          aria-label="Sign out"
          @click="session.signOut()"
        >
          ⎋
        </button>
      </nav>
      <router-view />
    </div>
    <nav class="bottom-nav" aria-label="Workspace">
      <router-link class="nav-item" active-class="active" :to="{ name: 'home' }">
        Overview
      </router-link>
      <router-link class="nav-item" active-class="active" :to="{ name: 'account' }">
        Account
      </router-link>
    </nav>
  </div>
</template>
