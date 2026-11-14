<template>
  <a href="#tiles" class="skip-link" @click.prevent="focusTiles">{{ t('home.skip') }}</a>
  <main>
    <header class="top">
      <h1>{{ t('app.name') }}</h1>
      <div class="lights">
        <StatusLight
          name="Twitch"
          :light="twitch"
          :aria-expanded="openId === 'connections'"
          :aria-controls="PANEL_ID"
          @click="openPanel('connections', $event.currentTarget)"
        />
        <StatusLight
          name="OBS"
          :light="obs"
          :aria-expanded="openId === 'connections'"
          :aria-controls="PANEL_ID"
          @click="openPanel('connections', $event.currentTarget)"
        />
      </div>
    </header>
    <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>

    <!-- Later slices add pending items; the list shows only when it has some. -->
    <section v-if="pending.length" aria-labelledby="pending-heading">
      <h2 id="pending-heading">{{ t('home.pending') }}</h2>
      <ul class="plain">
        <li v-for="p in pending" :key="p.id">
          <Button :label="p.text" text @click="openPanel(p.panel, $event.currentTarget)" />
        </li>
      </ul>
    </section>

    <section id="tiles" ref="tilesEl" tabindex="-1" :aria-label="t('home.tiles')">
      <p v-if="!modules">{{ loadFailed ? t('home.loadFailed') : t('app.loading') }}</p>
      <div v-else-if="!groups.length" class="stack">
        <p>{{ t('home.empty') }}</p>
        <!-- TODO(#12): enter arrange mode. Until then the module switches are the way to bring tiles back. -->
        <Button :label="t('home.arrange')" icon="pi pi-pencil" outlined @click="focusModules" />
      </div>
      <template v-for="g in groups" :key="g.group">
        <h2 :id="`group-${g.group}`">{{ t(`home.groups.${g.group}`) }}</h2>
        <ul class="plain grid" :aria-labelledby="`group-${g.group}`">
          <li v-for="tile in g.tiles" :key="tile.id">
            <HomeTile
              :name="moduleName(tile.id)"
              :icon="tile.icon"
              :line="lines[tile.id] ?? { state: 'loading' }"
              :attention="attention[tile.id]"
              :expanded="openId === tile.id"
              @click="openPanel(tile.id, $event.currentTarget)"
            />
          </li>
        </ul>
      </template>
    </section>

    <!-- TODO(#9): stand-in for the drawer; move these panels into it and drop this section once #9 merges. -->
    <section v-if="openId" :id="PANEL_ID" class="panel" aria-labelledby="panel-heading" @keydown.esc="closePanel">
      <div class="panel-head">
        <h2 id="panel-heading">{{ openId === 'connections' ? t('home.connections') : moduleName(openId) }}</h2>
        <Button id="panel-close" icon="pi pi-times" text :aria-label="t('app.close')" @click="closePanel" />
      </div>

      <template v-if="openId === 'connections'">
        <h3>{{ t('twitch.heading') }}</h3>
        <p v-if="!overview">{{ t('app.loading') }}</p>

        <form v-else-if="!overview.client_id || editingClientId" @submit.prevent="saveClientId">
          <i18n-t keypath="twitch.guide" tag="p">
            <template #url><code>http://localhost</code></template>
          </i18n-t>
          <label for="client-id">{{ t('twitch.clientId') }}</label>
          <InputText id="client-id" v-model="clientIdInput" autocomplete="off" required />
          <Button type="submit" :label="t('twitch.save')" :disabled="busy" />
          <Button v-if="overview.client_id" :label="t('twitch.cancel')" outlined @click="editingClientId = false" />
        </form>

        <template v-else>
          <div v-if="overview.status.state === 'signed_in'" class="stack">
            <p>{{ t('twitch.signedIn') }}<strong>{{ overview.status.login }}</strong></p>
            <Button :label="t('twitch.logout')" icon="pi pi-sign-out" outlined :disabled="busy" @click="logout" />
          </div>

          <div v-else-if="overview.status.state === 'pending'" class="stack">
            <p>{{ t('twitch.enterCode') }}</p>
            <p class="user-code" aria-live="polite">{{ overview.status.user_code }}</p>
            <Button :label="t('twitch.openActivate')" icon="pi pi-external-link" @click="openActivate" />
          </div>

          <div v-else class="stack">
            <p v-if="overview.status.state === 'failed'" role="alert">{{ errorMessage(overview.status.code) }}</p>
            <Button :label="t('twitch.login')" icon="pi pi-sign-in" :disabled="busy" @click="login" />
          </div>

          <Button :label="t('twitch.changeClientId')" text @click="startEditClientId" />
        </template>

        <h3>OBS</h3>
        <p>
          {{ t(obs.key) }}
          <template v-if="live.obs?.reason">— {{ errorMessage(live.obs.reason) }}</template>
        </p>
      </template>

      <template v-else-if="openId === 'chat'">
        <h3>{{ t('simulate.heading') }}</h3>
        <p>{{ t('simulate.hint') }}</p>
        <form @submit.prevent="simulateChat">
          <label for="sim-user">{{ t('simulate.userName') }}</label>
          <InputText id="sim-user" v-model="simUser" autocomplete="off" maxlength="25" />
          <label for="sim-text">{{ t('simulate.text') }}</label>
          <InputText id="sim-text" v-model="simText" autocomplete="off" maxlength="500" required />
          <Button type="submit" :label="t('simulate.send')" icon="pi pi-send" :disabled="busy" />
        </form>
      </template>
    </section>

    <!-- TODO(#10): moves to the settings page. -->
    <section aria-labelledby="modules-heading">
      <h2 id="modules-heading" ref="modulesHeading" tabindex="-1">{{ t('modules.heading') }}</h2>
      <ul class="plain">
        <li v-for="m in modules" :key="m.id" class="module">
          <Checkbox :input-id="`module-${m.id}`" :model-value="m.enabled" binary :disabled="busy" @update:model-value="toggleModule(m)" />
          <label :for="`module-${m.id}`">{{ moduleName(m.id) }}</label>
        </li>
      </ul>
    </section>
  </main>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import Button from 'primevue/button'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import {
  type Line,
  PANEL_ID,
  errorToast,
  obsLight,
  successToast,
  tileGroups,
  twitchLight,
  useLiveEvents,
} from './home'
import HomeTile from './HomeTile.vue'
import StatusLight from './StatusLight.vue'

const { t, te } = useI18n()
const toast = useToast()

const overview = ref<Schemas['Overview']>()
const overviewFailed = ref(false)
const modules = ref<Schemas['ModuleState'][]>()
const loadFailed = ref(false)
const clientIdInput = ref('')
const editingClientId = ref(false)
const busy = ref(false)
const simUser = ref('')
const simText = ref('')
const templateError = ref<string | null>(null)
const openId = ref<string>()
const tilesEl = ref<HTMLElement>()
const modulesHeading = ref<HTMLElement>()
let opener: HTMLElement | undefined
let timer: number | undefined

const live = useLiveEvents()
const twitch = computed(() => twitchLight(overview.value, overviewFailed.value))
const obs = computed(() => obsLight(live.value))
const groups = computed(() => tileGroups(modules.value ?? []))
const pending = computed<{ id: string; text: string; panel: string }[]>(() => [])

const lines = computed<Record<string, Line>>(() => {
  const { status, lastChat } = live.value
  return {
    chat:
      status === 'open'
        ? { state: 'ready', text: lastChat ? `${lastChat.payload.userName}：${lastChat.payload.text}` : t('home.chatEmpty') }
        : { state: status },
  }
})
const attention = computed<Record<string, boolean>>(() => ({ chat: templateError.value !== null }))

const moduleName = (id: string) => (te(`modules.names.${id}`) ? t(`modules.names.${id}`) : id)

async function refreshOverview() {
  try {
    overview.value = await api(client.GET('/api/admin/twitch'))
    overviewFailed.value = false
  } catch (e) {
    overviewFailed.value = true
    throw e
  }
}

async function refresh() {
  await refreshOverview()
  modules.value = await api(client.GET('/api/admin/modules'))
  await refreshTemplate()
}

async function refreshTemplate() {
  templateError.value = (await api(client.GET('/api/admin/templates/chat'))).error
}

async function run(action: () => Promise<unknown>) {
  busy.value = true
  try {
    await action()
    await refresh()
  } catch (e) {
    loadFailed.value = !modules.value
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))
  } finally {
    busy.value = false
  }
}

const saveClientId = () =>
  run(async () => {
    await api(client.PUT('/api/admin/twitch/client-id', { body: { client_id: clientIdInput.value } }))
    editingClientId.value = false
  })
const login = () => run(() => api(client.POST('/api/admin/twitch/login')))
const logout = () => run(() => api(client.POST('/api/admin/twitch/logout')))
const toggleModule = (m: Schemas['ModuleState']) =>
  run(() =>
    api(client.PUT('/api/admin/modules/{id}', { params: { path: { id: m.id } }, body: { enabled: !m.enabled } })),
  )
const simulateChat = () =>
  run(async () => {
    await api(client.POST('/api/admin/simulate/chat', { body: { userName: simUser.value, text: simText.value } }))
    simText.value = ''
    toast.add(successToast(t('simulate.sent')))
  })

async function openActivate() {
  try {
    await invoke('open_twitch_activate')
  } catch (code) {
    toast.add(errorToast(errorMessage(String(code))))
  }
}

function startEditClientId() {
  clientIdInput.value = overview.value?.client_id ?? ''
  editingClientId.value = true
}

// TODO(#9): replace with the drawer's open/close (it also owns focus return and submit-before-close).
async function openPanel(id: string, el: EventTarget | null) {
  if (openId.value === id) return closePanel()
  openId.value = id
  opener = el as HTMLElement
  await nextTick()
  document.getElementById('panel-close')?.focus()
}

function closePanel() {
  openId.value = undefined
  opener?.focus()
}

const focusTiles = () => tilesEl.value?.focus()
const focusModules = () => modulesHeading.value?.focus()

onMounted(async () => {
  await run(async () => {})
  // Statuses and the template are polled, so a lost sign-in or a broken template edit shows up without a reload.
  // Until the first full load succeeds, retry all of it, so one startup failure does not leave the tiles empty.
  timer = window.setInterval(() => {
    const poll = modules.value ? Promise.all([refreshOverview(), refreshTemplate()]) : refresh()
    poll.catch(() => {})
  }, 2000)
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>

<style scoped>
.top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem 1rem;
}

.lights {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.plain {
  list-style: none;
  padding: 0;
}

.grid {
  display: grid;
  /* 12.5rem = 200px at the default font size. */
  grid-template-columns: repeat(auto-fill, minmax(12.5rem, 1fr));
  gap: 0.75rem;
}

@media (max-width: 599.98px) {
  .grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

#tiles:focus {
  outline: none;
}

.panel {
  margin-top: 1.5rem;
  padding: 0 1rem 1rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
}

.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.module {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.user-code {
  font-size: 1.5rem;
  font-weight: 600;
  letter-spacing: 0.1em;
}
</style>
