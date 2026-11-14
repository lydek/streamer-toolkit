<template>
  <dialog ref="dialog" aria-labelledby="close-hint-message" @close="onClose">
    <p id="close-hint-message">{{ t('closeHint.message') }}</p>
    <div class="choice">
      <Checkbox v-model="dontAskAgain" input-id="close-hint-dont-ask" binary />
      <label for="close-hint-dont-ask">{{ t('closeHint.dontAskAgain') }}</label>
    </div>
    <Button :label="t('closeHint.ok')" autofocus @click="hide" />
  </dialog>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import { onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

// Rust asks when the admin window's ✕ is pressed (docs/architecture.md §6): `true` shows the hint, `false` closes it.
const { t } = useI18n()
const dialog = ref<HTMLDialogElement>()
const dontAskAgain = ref(false)
let unlisten: UnlistenFn | undefined

onMounted(async () => {
  unlisten = await listen<boolean>('close-hint', ({ payload: show }) => {
    const d = dialog.value!
    if (!show) {
      if (d.open) d.close('dismissed')
      return
    }
    if (!d.open) {
      d.returnValue = ''
      dontAskAgain.value = false
      d.showModal()
    }
    void invoke('close_hint', { action: 'shown' })
  })
})
onUnmounted(() => unlisten?.())

function hide() {
  dialog.value!.close('hide')
  void invoke('close_hint', { action: { hide: { dont_ask_again: dontAskAgain.value } } })
}

// An empty return value means the dialog closed on its own (Esc), so the window stays.
function onClose() {
  if (dialog.value!.returnValue === '') void invoke('close_hint', { action: 'cancel' })
}
</script>

<style scoped>
dialog {
  max-width: min(28rem, calc(100vw - 2rem));
  border: 1px solid var(--p-content-border-color);
  border-radius: var(--p-content-border-radius);
  background: var(--p-content-background);
  color: var(--p-text-color);
}

dialog[open] {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.75rem;
}

dialog::backdrop {
  background: rgb(0 0 0 / 0.6);
}

p {
  margin: 0;
}

.choice {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
</style>
