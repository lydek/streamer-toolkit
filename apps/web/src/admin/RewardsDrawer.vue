<template>
  <AppDrawer id="rewards" :title="title" full-page="/rewards">
    <template #actions>
      <DrawerNotice />
    </template>
    <template #now>
      <!-- As the full page: the toolkit's own rewards can be paused; those made in the Twitch dashboard are read-only. -->
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <p v-else-if="!rewards">{{ t('app.loading') }}</p>
      <p v-else-if="!rewards.length">{{ t('rewards.empty') }}</p>
      <ul v-else :key="resets" class="rewards">
        <li v-for="r in rewards" :key="r.id" :data-reward="r.id">
          <span class="name">{{ r.title }}</span>
          <ToggleSwitch
            v-if="r.manageable"
            :model-value="r.is_paused"
            :disabled="busy === r.id || !r.is_enabled"
            :aria-label="t('rewards.pauseLabel', { name: r.title })"
            @update:model-value="pause(r, $event)"
          />
          <span v-if="r.manageable && !r.is_enabled" class="muted">{{ t('rewards.off') }}</span>
          <span v-else-if="!r.manageable" class="muted">{{ t('rewards.dashboardOnly') }}</span>
        </li>
      </ul>
      <RouterLink :to="{ path: '/rewards', query: { new: '1' } }" class="add">
        <i class="pi pi-plus" aria-hidden="true"></i>{{ t('rewards.add') }}
      </RouterLink>
    </template>
  </AppDrawer>
</template>

<script setup lang="ts">
import ToggleSwitch from 'primevue/toggleswitch'
import { ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import AppDrawer from './AppDrawer.vue'
import { useDrawer } from './drawer'
import DrawerNotice from './DrawerNotice.vue'
import { useHomeDrawer } from './homeDrawer'

defineProps<{ title: string }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
const drawer = useDrawer()
const rewards = shallowRef<Schemas['ChannelReward'][]>()
const loadFailed = ref('')
const busy = ref<string>()
// A refused change redraws the switches from what Twitch saved.
const resets = ref(0)

async function load() {
  try {
    rewards.value = await api(client.GET('/api/admin/rewards'))
    loadFailed.value = ''
  } catch (e) {
    // The same words as the full page (a missing Twitch permission included).
    loadFailed.value = e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)
  }
}
// Read each time the drawer opens, so it matches Twitch.
watch(drawer.current, (id) => id === 'rewards' && void load(), { immediate: true })

async function pause(r: Schemas['ChannelReward'], is_paused: boolean) {
  busy.value = r.id
  try {
    await api(client.PATCH('/api/admin/rewards/{id}', { params: { path: { id: r.id } }, body: { is_paused } }))
  } catch (e) {
    resets.value++
    failed(e)
  } finally {
    await load()
    busy.value = undefined
  }
}
</script>

<style scoped>
.rewards {
  display: grid;
  gap: 0.375rem;
  margin: 0 0 0.75rem;
  padding: 0;
  list-style: none;
}

.rewards li {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.name {
  min-width: 0;
  margin-right: auto;
  overflow-wrap: anywhere;
}

.muted {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.add {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  color: var(--p-primary-color);
}
</style>
