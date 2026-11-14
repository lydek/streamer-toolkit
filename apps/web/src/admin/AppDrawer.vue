<template>
  <!-- Wider than 1000px: a non-modal aside beside the tiles. Otherwise a modal dialog. Teleport moves the same
       content between them, so inputs, component state and listeners survive (see drawer.ts for usage). -->
  <aside ref="aside" class="drawer-aside" :hidden="!isOpen || modal" :aria-labelledby="titleId" />
  <dialog
    ref="dialog"
    class="drawer-dialog"
    :aria-labelledby="titleId"
    @cancel.prevent="close"
    @close="onDialogClose"
    @pointerdown="onPointerdown"
    @click="onClick"
  />
  <Teleport v-if="isOpen && mounted" :to="(modal ? dialog : aside)!">
    <div ref="content" class="drawer-content">
      <header class="drawer-header">
        <h2 :id="titleId">{{ title }}</h2>
        <Button data-drawer-close icon="pi pi-times" text rounded :aria-label="t('drawer.close')" autofocus @click="close" />
      </header>
      <div v-if="$slots.actions" class="drawer-actions"><slot name="actions" /></div>
      <section v-if="$slots.now || error" :aria-labelledby="`${titleId}-now`">
        <h3 :id="`${titleId}-now`">{{ t('drawer.now') }}</h3>
        <p v-if="error" role="alert">{{ error }}</p>
        <slot name="now" />
      </section>
      <section v-if="$slots.settings" :aria-labelledby="`${titleId}-settings`">
        <h3 :id="`${titleId}-settings`">{{ t('drawer.settings') }}</h3>
        <slot name="settings" />
      </section>
      <RouterLink v-if="fullPage" :to="fullPage">{{ t('drawer.fullPage') }}</RouterLink>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { computed, nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { RouteLocationRaw } from 'vue-router'
import { useDrawer } from './drawer'

const props = defineProps<{ id: string; title: string; fullPage?: RouteLocationRaw }>()

const { t } = useI18n()
const { current, error, close } = useDrawer()
const titleId = useId()
const aside = ref<HTMLElement>()
const dialog = ref<HTMLDialogElement>()
const content = ref<HTMLElement>()
const mounted = ref(false)
const isOpen = computed(() => current.value === props.id)
let pressedBackdrop = false
// Set before every close() this component makes, so the close event it fires is not taken for the user's.
let closingDialog = false

const query = window.matchMedia('(max-width: 1000px)')
const modal = ref(query.matches)
const onQuery = (e: MediaQueryListEvent) => (modal.value = e.matches)

function openDialog() {
  if (dialog.value && !dialog.value.open) dialog.value.showModal()
}

function closeDialog() {
  if (!dialog.value?.open) return
  closingDialog = true
  dialog.value.close()
}

// The content fills the dialog box, so a target of the dialog itself is its backdrop. Both ends of the click
// must be there: a text selection dragged out of the content must not close it.
function onPointerdown(e: PointerEvent) {
  pressedBackdrop = e.target === dialog.value
}

function onClick(e: MouseEvent) {
  if (pressedBackdrop && e.target === dialog.value) close()
}

function focusClose() {
  content.value?.querySelector<HTMLElement>('[data-drawer-close]')?.focus()
}

// A close request the dialog handled itself (e.g. a second Esc with no click in between skips `cancel`).
async function onDialogClose() {
  if (closingDialog) return void (closingDialog = false)
  if (isOpen.value && !(await close()) && isOpen.value && modal.value) openDialog()
}

function onKeydown(e: KeyboardEvent) {
  // Components that handle Esc themselves (an open Select) prevent the default; leave that Esc to them.
  if (e.key === 'Escape' && !e.defaultPrevented && isOpen.value && !modal.value) close()
}

watch(
  isOpen,
  (open) => {
    if (!open) return closeDialog()
    if (modal.value) openDialog()
    focusClose()
  },
  { flush: 'post' },
)

// Runs before the Teleport moves: record focus and scroll positions, which moving DOM nodes resets.
watch(modal, async (toModal) => {
  if (!isOpen.value || !content.value) return
  const root = content.value
  const focused = root.contains(document.activeElement) ? (document.activeElement as HTMLElement) : null
  const scrolled = [root, ...root.querySelectorAll<HTMLElement>('*')]
    .filter((el) => el.scrollTop > 0)
    .map((el) => [el, el.scrollTop] as const)
  await nextTick()
  if (toModal) openDialog()
  else closeDialog()
  for (const [el, top] of scrolled) el.scrollTop = top
  focused?.focus({ preventScroll: true })
})

onMounted(() => {
  mounted.value = true
  query.addEventListener('change', onQuery)
  document.addEventListener('keydown', onKeydown)
})
onUnmounted(() => {
  query.removeEventListener('change', onQuery)
  document.removeEventListener('keydown', onKeydown)
})
</script>

<style scoped>
.drawer-aside {
  flex: none;
  width: 380px;
  position: sticky;
  top: 0;
  max-height: 100vh;
  display: flex;
  border-left: 1px solid var(--p-surface-700);
  background: var(--p-surface-900);
}

.drawer-aside[hidden] {
  display: none;
}

.drawer-dialog {
  margin: 0 0 0 auto;
  padding: 0;
  width: min(380px, 100%);
  max-width: 100%;
  height: 100%;
  max-height: 100%;
  border: none;
  border-left: 1px solid var(--p-surface-700);
  background: var(--p-surface-900);
  color: inherit;
}

.drawer-dialog[open] {
  display: flex;
}

.drawer-dialog::backdrop {
  background: rgb(0 0 0 / 0.6);
}

/* Below 600px: a bottom sheet. */
@media (max-width: 599.98px) {
  .drawer-dialog {
    margin: auto 0 0;
    width: 100%;
    height: auto;
    max-height: 85vh;
    border-left: none;
    border-top: 1px solid var(--p-surface-700);
    border-radius: 6px 6px 0 0;
  }
}

.drawer-content {
  flex: 1;
  overflow-y: auto;
  padding: 1rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.drawer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
}

.drawer-header h2,
.drawer-content h3 {
  margin: 0;
}

.drawer-content h3 {
  font-size: 1rem;
}

.drawer-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
