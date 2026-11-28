<template>
  <AppDrawer id="commands" :title="title" full-page="/commands">
    <template #actions>
      <DrawerNotice />
    </template>
    <template #now>
      <!-- The timers and the automatic greetings, each switched on or off as on their pages. -->
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <template v-else>
        <h3>{{ t('pages.timers.name') }}</h3>
        <p v-if="!timers">{{ t('app.loading') }}</p>
        <p v-else-if="!timers.length" class="muted">{{ t('bot.timers.empty') }}</p>
        <ul v-else :key="resets" class="rows">
          <li v-for="tm in timers" :key="tm.id">
            <span class="name">{{ tm.name }}</span>
            <ToggleSwitch
              :model-value="tm.enabled"
              :disabled="busy"
              :aria-label="t('bot.timers.enableLabel', { name: tm.name })"
              @update:model-value="setTimer(tm, $event)"
            />
          </li>
        </ul>
        <h3>{{ t('pages.greetings.name') }}</h3>
        <p v-if="!greetings">{{ t('app.loading') }}</p>
        <ul v-else :key="resets" class="rows">
          <li v-for="row in GREETINGS" :key="row">
            <span class="name">{{ t(`bot.greetings.${row}.name`) }}</span>
            <ToggleSwitch
              :model-value="greetings[KEYS[row]].enabled"
              :disabled="busy"
              :aria-label="t('bot.greetings.enabled', { name: t(`bot.greetings.${row}.name`) })"
              @update:model-value="setGreeting(row, $event)"
            />
          </li>
        </ul>
      </template>
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

type Timer = Schemas['BotTimerView']
type Greetings = Schemas['BotGreetings']
const GREETINGS = ['follow', 'firstChat', 'returning', 'sub', 'bits', 'raid'] as const
const KEYS = { follow: 'follow', firstChat: 'first_chat', returning: 'returning', sub: 'sub', bits: 'bits', raid: 'raid' } as const

defineProps<{ title: string }>()
const { t } = useI18n()
const { failed } = useHomeDrawer()
const drawer = useDrawer()
const timers = shallowRef<Timer[]>()
const greetings = ref<Greetings>()
const loadFailed = ref('')
const busy = ref(false)
// A refused change redraws the switches from what is saved.
const resets = ref(0)

async function load() {
  try {
    ;[timers.value, greetings.value] = await Promise.all([api(client.GET('/api/admin/bot/timers')), api(client.GET('/api/admin/bot/greetings'))])
    loadFailed.value = ''
  } catch (e) {
    // The same words as the pages (the bot being off or a missing Twitch permission included).
    loadFailed.value = e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)
  }
}
watch(drawer.current, (id) => id === 'commands' && void load(), { immediate: true })

/** Runs a change, then reads both lists again, so a refused one shows what is saved. */
async function change(action: () => Promise<unknown>) {
  busy.value = true
  try {
    await action()
  } catch (e) {
    resets.value++
    failed(e)
  } finally {
    await load()
    busy.value = false
  }
}

const setTimer = (tm: Timer, enabled: boolean) =>
  change(() =>
    api(
      client.PUT('/api/admin/bot/timers/{id}', {
        params: { path: { id: tm.id } },
        body: { name: tm.name, message: tm.message, every_minutes: tm.every_minutes, min_chat_lines: tm.min_chat_lines, enabled },
      }),
    ),
  )

const setGreeting = (row: (typeof GREETINGS)[number], enabled: boolean) =>
  change(() => {
    const body = { ...greetings.value!, [KEYS[row]]: { ...greetings.value![KEYS[row]], enabled } }
    return api(client.PUT('/api/admin/bot/greetings', { body }))
  })
</script>

<style scoped>
h3 {
  margin: 0.75rem 0 0.375rem;
  font-size: 0.9375rem;
}

.rows {
  display: grid;
  gap: 0.375rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.rows li {
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
}
</style>
