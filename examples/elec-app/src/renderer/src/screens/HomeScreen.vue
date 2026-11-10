<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { TASKS } from '../session'

const completed = ref(new Set<number>([0]))
const statsEl = ref<HTMLElement | null>(null)
const stack = ref(false)
let observer: ResizeObserver | undefined

function toggle(index: number, checked: boolean): void {
  const next = new Set(completed.value)
  if (checked) next.add(index)
  else next.delete(index)
  completed.value = next
}

const wins = computed(() => `${completed.value.size}/3`)

onMounted(() => {
  const node = statsEl.value
  if (!node) return
  const update = (): void => {
    const root = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
    stack.value = node.clientWidth < 640 || root > 24
  }
  update()
  observer = new ResizeObserver(update)
  observer.observe(node)
})

onUnmounted(() => observer?.disconnect())
</script>

<template>
  <main id="orbit-viewport" class="workspace" data-testid="orbit-viewport">
    <div class="workspace-inner">
      <div class="eyebrow">YOUR WORKSPACE</div>
      <h1 class="headline">A fresh start, Alex.</h1>
      <p class="lede">Let’s make a little progress on what matters.</p>
      <section class="hero-card">
        <h2>Find your flow.</h2>
        <p>Big ideas begin with small, intentional steps. Your next one is right here.</p>
        <router-link
          id="view-profile-button"
          class="btn btn-filled"
          data-testid="view-profile-button"
          :to="{ name: 'account' }"
        >
          View your profile
        </router-link>
      </section>
      <div ref="statsEl" class="stats" :class="{ stack }">
        <div class="surface-card">
          <div>{{ wins }}</div>
          <div>Small wins</div>
        </div>
        <div class="surface-card">
          <div>20 min</div>
          <div>Time for yourself</div>
        </div>
        <div class="surface-card">
          <div>Today</div>
          <div>A good day to begin</div>
        </div>
      </div>
      <div class="surface-card">
        <h2>A little plan for today</h2>
        <p>Check things off. Enjoy the small wins.</p>
        <label
          v-for="(task, index) in TASKS"
          :id="`task-${index + 1}-checkbox`"
          :key="task.title"
          class="task"
          :class="{ done: completed.has(index) }"
        >
          <input
            type="checkbox"
            :data-testid="`task-${index + 1}-checkbox`"
            :checked="completed.has(index)"
            @change="toggle(index, ($event.target as HTMLInputElement).checked)"
          />
          <span class="task-copy">
            <span class="task-title">{{ task.title }}</span>
            <small>{{ task.detail }}</small>
          </span>
        </label>
        <div
          class="progress theme"
          role="progressbar"
          :aria-label="`${completed.size} of 3 tasks complete`"
          :aria-valuenow="completed.size"
          aria-valuemin="0"
          aria-valuemax="3"
        >
          <span :style="{ width: `${(completed.size / 3) * 100}%` }" />
        </div>
      </div>
      <p class="tagline">Your pace. Your space. Your orbit.</p>
    </div>
  </main>
</template>
