<template>
  <AppDrawer id="songs" :title="title" full-page="/songs">
    <template #actions>
      <DrawerNotice />
    </template>
    <template #now>
      <!-- The song being sung with 唱完了, and the next three, as on the song request page. -->
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <p v-else-if="!board">{{ t('app.loading') }}</p>
      <template v-else>
        <div v-if="board.singing" class="now" data-singing>
          <p class="title">
            <b>{{ board.singing.title }}</b> <span class="muted">{{ board.singing.artist }}</span>
          </p>
          <p v-if="board.singing.viewer" class="muted">{{ t('songs.by', { viewer: board.singing.viewer }) }}</p>
          <Button :label="t('songs.finish')" icon="pi pi-check" :disabled="busy" @click="finish" />
        </div>
        <p v-else class="muted">{{ t('songs.nothingNow') }}</p>
        <h3>{{ t('songs.queue') }}</h3>
        <p v-if="!board.queued.length" class="muted">{{ t('songs.emptyQueue') }}</p>
        <ol v-else class="next">
          <li v-for="r in board.queued.slice(0, 3)" :key="r.id">{{ r.title }}</li>
        </ol>
      </template>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import { ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import AppDrawer from './AppDrawer.vue'
import { useDrawer } from './drawer'
import DrawerNotice from './DrawerNotice.vue'
import { useHomeDrawer } from './homeDrawer'

/** `changed` goes up when the live socket says the requests changed (or reconnects). */
const props = defineProps<{ title: string; changed: number }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
const drawer = useDrawer()
const board = shallowRef<Schemas['RequestBoard']>()
const loadFailed = ref('')
const busy = ref(false)

async function load() {
  try {
    board.value = await api(client.GET('/api/admin/song-requests'))
    loadFailed.value = ''
  } catch (e) {
    loadFailed.value = e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)
  }
}
// Read each time the drawer opens.
watch(drawer.current, (id) => id === 'songs' && void load(), { immediate: true })
// A request from chat or another window changes the list while the drawer is open.
watch(() => props.changed, () => drawer.current.value === 'songs' && void load())

async function finish() {
  busy.value = true
  try {
    await api(client.POST('/api/admin/song-requests/finish'))
  } catch (e) {
    failed(e)
  } finally {
    await load()
    busy.value = false
  }
}
</script>

<style scoped>
.now {
  display: grid;
  justify-items: start;
  gap: 0.25rem;
}

.now p {
  margin: 0;
}

.title b {
  font-size: 1.125rem;
}

.muted {
  color: var(--p-text-muted-color);
}

h3 {
  margin: 0.75rem 0 0.375rem;
  font-size: 0.9375rem;
}

.next {
  margin: 0;
  padding-inline-start: 1.25rem;
}
</style>
