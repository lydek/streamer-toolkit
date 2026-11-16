<template>
  <dialog ref="dialog" aria-labelledby="checkin-disable-title" @close="onClose">
    <h2 id="checkin-disable-title">{{ t('checkin.disable.title') }}</h2>
    <p>{{ t('checkin.disable.pause') }}</p>
    <p id="checkin-close-warning" role="alert">
      {{ t('checkin.disable.closeWarning') }}
      <template v-if="pending > 0">{{ t('checkin.disable.pendingSkipped', { n: pending }) }}</template>
    </p>
    <div class="buttons">
      <Button :label="t('checkin.disable.pauseButton')" autofocus @click="choose('pause')" />
      <Button
        :label="t('checkin.disable.closeButton')"
        severity="danger"
        outlined
        aria-describedby="checkin-close-warning"
        @click="choose('close')"
      />
      <Button :label="t('twitch.cancel')" text @click="dialog!.close()" />
    </div>
  </dialog>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

// Disabling check-ins: "pause" keeps redemptions for later, "close" skips them (docs/architecture.md §4).
const { t } = useI18n()
const emit = defineEmits<{ choose: ['pause' | 'close'] }>()
const dialog = ref<HTMLDialogElement>()
const pending = ref(0)

/** `waiting`: redemptions already kept (while paused); closing skips them too. */
function open(waiting = 0) {
  pending.value = waiting
  dialog.value!.returnValue = ''
  dialog.value!.showModal()
}

function choose(mode: 'pause' | 'close') {
  dialog.value!.close(mode)
}

// Esc and Cancel close with an empty return value: nothing changes.
function onClose() {
  const mode = dialog.value!.returnValue
  if (mode === 'pause' || mode === 'close') emit('choose', mode)
}

defineExpose({ open })
</script>

<style scoped>
dialog {
  max-width: min(30rem, calc(100vw - 2rem));
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

h2,
p {
  margin: 0;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
