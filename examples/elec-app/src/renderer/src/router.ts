import { createRouter, createWebHistory } from 'vue-router'
import { watch } from 'vue'
import { destinationFrom, loginRedirect } from './session'
import { session } from './session-state'
import AuthScreen from './screens/AuthScreen.vue'
import WorkspaceShell from './screens/WorkspaceShell.vue'
import HomeScreen from './screens/HomeScreen.vue'
import AccountScreen from './screens/AccountScreen.vue'
import NotFound from './screens/NotFound.vue'

function queryFrom(value: unknown): string | null {
  return typeof value === 'string' ? value : null
}

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: AuthScreen,
      props: { resetPassword: false }
    },
    {
      path: '/forgot-password',
      name: 'forgot-password',
      component: AuthScreen,
      props: { resetPassword: true }
    },
    {
      path: '/',
      component: WorkspaceShell,
      children: [
        { path: 'home', name: 'home', component: HomeScreen },
        { path: 'account', name: 'account', component: AccountScreen }
      ]
    },
    { path: '/:pathMatch(.*)*', name: 'missing', component: NotFound }
  ]
})

router.beforeEach((to) => {
  if (to.path === '/') return '/home'
  const isAuth = to.path === '/login' || to.path === '/forgot-password'
  const isProtected = to.path === '/home' || to.path === '/account'
  if (!session.signedIn && isProtected) {
    return loginRedirect(to.fullPath)
  }
  if (session.signedIn && isAuth) {
    return destinationFrom(queryFrom(to.query.from))
  }
  return true
})

watch(
  () => session.signedIn,
  () => {
    void router.replace(router.currentRoute.value.fullPath)
  }
)

export default router
