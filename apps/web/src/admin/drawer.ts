import { getCurrentScope, nextTick, onScopeDispose, readonly, ref, watch, type Ref } from 'vue'
import { errorMessage } from '../i18n'
import { ApiError } from './api'

/*
 * Drawer state (docs/decisions.md 抽屜元件). One drawer at a time, shared by the whole admin window.
 *
 *   const drawer = useDrawer()
 *   <button data-drawer-tile="chat" @click="drawer.open('chat', $event.currentTarget)">…</button>
 *   <AppDrawer id="chat" :title="…" full-page="/chat"> <template #now>…</template> </AppDrawer>
 *
 * - `open(id, opener)` / `close()` submit changed settings first and resolve `false` (drawer stays as it was)
 *   when one fails, so its error stays visible. The router guard calls `close()` on every navigation.
 * - Focus returns to the opener on close, else to `[data-drawer-tile="<id>"]`, the first `[data-drawer-tile]`,
 *   then `[data-drawer-arrange]` ("Arrange tiles"). Tiles and that button carry these attributes.
 * - Settings inside a drawer use `useDrawerSetting`; PrimeVue overlays inside it (Select, DatePicker…) need
 *   `appendTo="self"`, because a modal `<dialog>` is in the top layer and covers anything appended to `body`.
 */

type Submit = () => Promise<boolean>

const current = ref<string | null>(null)
const error = ref('')
const submits = new Set<Submit>()
let opener: HTMLElement | null = null
// Opens and closes run one after another, so a second Esc during a slow submit cannot submit twice.
let queue: Promise<unknown> = Promise.resolve()

function serial<T>(task: () => Promise<T>): Promise<T> {
  const run = queue.then(task)
  queue = run.catch(() => {})
  return run
}

/** Runs every submit in order; all of them run even after a failure, so each changed field is tried. */
export async function submitAll(list: Iterable<Submit>): Promise<boolean> {
  let ok = true
  for (const submit of list) ok = (await submit()) && ok
  return ok
}

function usable(el: Element | null | undefined): el is HTMLElement {
  return !!el && el.isConnected && !(el as HTMLButtonElement).disabled && !el.closest('[hidden], [inert]')
}

/** Where focus goes when drawer `id` closes: the opener, its tile, the first tile, then "Arrange tiles". */
export function focusTarget(root: ParentNode, id: string, from: HTMLElement | null): HTMLElement | null {
  const tiles = [...root.querySelectorAll('[data-drawer-tile]')].filter(usable)
  const candidates = [
    from,
    tiles.find((el) => el.getAttribute('data-drawer-tile') === id),
    tiles[0],
    root.querySelector('[data-drawer-arrange]'),
  ]
  return candidates.find(usable) ?? null
}

/** Switches to drawer `id`, submitting the open drawer's changed settings first. */
function open(id: string, from?: EventTarget | null): Promise<boolean> {
  return serial(async () => {
    if (current.value === id) return true
    if (current.value && !(await submitAll(submits))) return false
    error.value = ''
    opener = from instanceof HTMLElement ? from : null
    current.value = id
    return true
  })
}

/** Closes the open drawer after submitting its changed settings; `true` when nothing is open. */
function close(): Promise<boolean> {
  return serial(async () => {
    const id = current.value
    if (!id) return true
    if (!(await submitAll(submits))) return false
    error.value = ''
    current.value = null
    await nextTick()
    focusTarget(document, id, opener)?.focus()
    opener = null
    return true
  })
}

export function useDrawer() {
  return { current: readonly(current), error: readonly(error), open, close }
}

/** Registers a submit that `open`/`close` run first; it must resolve `false` on failure. */
export function onDrawerSubmit(submit: Submit): () => void {
  submits.add(submit)
  const remove = () => submits.delete(submit)
  if (getCurrentScope()) onScopeDispose(remove)
  return remove
}

/**
 * An immediately applied drawer setting (no Save button): bind `value`, call `commit()` on toggle, or on Enter
 * and blur for text. A failed save restores the saved value and shows the error at the top of the drawer.
 */
export function useDrawerSetting<T>(saved: Ref<T>, save: (value: T) => Promise<unknown>) {
  const value = ref(saved.value) as Ref<T>
  watch(saved, (v) => (value.value = v))
  async function apply(): Promise<boolean> {
    const next = value.value
    if (Object.is(next, saved.value)) return true
    try {
      await save(next)
      saved.value = next
      error.value = ''
      return true
    } catch (e) {
      value.value = saved.value
      error.value = e instanceof ApiError ? errorMessage(e.code) : String(e)
      return false
    }
  }
  // Chained, so blur followed at once by Esc saves once: the second call sees the value already saved.
  let chain = Promise.resolve(true)
  const commit = () => (chain = chain.then(apply))
  onDrawerSubmit(commit)
  return { value, commit }
}
