import { type InjectionKey, type Ref, inject } from 'vue'

/** A result shown inside the open drawer (toasts sit under a modal drawer). */
export type DrawerNotice = { text: string; error: boolean }

/** What the home page shares with its drawers: one notice, and its run / notify / busy, so a drawer's action
 *  refreshes home and reports where the streamer is looking. */
export interface HomeDrawerContext {
  notice: Ref<DrawerNotice | undefined>
  busy: Ref<boolean>
  /** Runs an action, then refreshes home; a failure shows in the drawer (or as a toast when none is modal). */
  run: (action: () => Promise<unknown>) => Promise<void>
  notify: (text: string, error: boolean) => void
  /** Shows a failed drawer action as the drawer's notice. */
  failed: (e: unknown) => void
}

export const HOME_DRAWER: InjectionKey<HomeDrawerContext> = Symbol('home drawer')

export function useHomeDrawer(): HomeDrawerContext {
  const ctx = inject(HOME_DRAWER)
  if (!ctx) throw new Error('a home drawer outside the home page')
  return ctx
}
