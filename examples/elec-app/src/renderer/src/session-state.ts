import { reactive } from 'vue'
import { authenticate } from './session'

export const session = reactive({
  signedIn: false,
  async signIn(email: string, password: string): Promise<boolean> {
    const success = await authenticate(email, password)
    if (success) this.signedIn = true
    return success
  },
  signOut(): void {
    this.signedIn = false
  }
})
