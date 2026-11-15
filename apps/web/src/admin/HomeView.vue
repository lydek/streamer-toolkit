<template>
  <a href="#tiles" class="skip-link" @click.prevent="focusTiles">{{ t('home.skip') }}</a>
  <div class="layout">
    <main>
      <header class="top">
        <h1>{{ t('app.name') }}</h1>
        <div class="lights">
          <StatusLight
            name="Twitch"
            :light="twitch"
            :aria-expanded="drawer.current.value === 'connections'"
            :aria-controls="drawerId('connections')"
            @click="toggle('connections', $event.currentTarget)"
          />
          <StatusLight
            name="OBS"
            :light="obs"
            :aria-expanded="drawer.current.value === 'connections'"
            :aria-controls="drawerId('connections')"
            @click="toggle('connections', $event.currentTarget)"
          />
          <Button v-if="arranging" ref="doneButton" :label="t('arrange.done')" icon="pi pi-check" @click="leaveArrange" />
          <Button
            v-else
            ref="arrangeButton"
            icon="pi pi-pencil"
            severity="secondary"
            text
            :aria-label="t('home.arrange')"
            :disabled="!modules"
            @click="enterArrange"
          />
          <Button
            icon="pi pi-cog"
            text
            rounded
            :aria-label="t('settings.title')"
            data-page-opener="settings"
            @click="router.push('/settings')"
          />
        </div>
      </header>
      <p v-if="templateError" role="alert">{{ errorMessage(templateError) }}</p>

      <!-- Later slices add pending items; the list shows only when it has some. -->
      <section v-if="pending.length" aria-labelledby="pending-heading">
        <h2 id="pending-heading">{{ t('home.pending') }}</h2>
        <ul class="plain">
          <li v-for="p in pending" :key="p.id">
            <Button :label="p.text" text @click="toggle(p.panel, $event.currentTarget)" />
          </li>
        </ul>
      </section>

      <section id="tiles" ref="tilesEl" tabindex="-1" :aria-label="t('home.tiles')">
        <p v-if="!modules">{{ loadFailed ? t('home.loadFailed') : t('app.loading') }}</p>
        <ArrangeTiles
          v-else-if="arranging"
          :saved="savedTiles"
          :save="saveTiles"
          :modules="modules"
          :name="moduleName"
        />
        <div v-else-if="!groups.length" class="stack">
          <p>{{ t('home.empty') }}</p>
          <Button :label="t('home.arrange')" icon="pi pi-pencil" outlined data-drawer-arrange @click="enterArrange" />
        </div>
        <template v-for="g in groups" v-else :key="g.group">
          <h2 :id="`group-${g.group}`">{{ t(`home.groups.${g.group}`) }}</h2>
          <ul class="plain grid" :aria-labelledby="`group-${g.group}`">
            <li v-for="tile in g.tiles" :key="tile.id">
              <HomeTile
                :id="tile.id"
                :name="moduleName(tile.id)"
                :icon="tile.icon"
                :line="lines[tile.id] ?? { state: 'loading' }"
                :attention="attention[tile.id]"
                :expanded="drawer.current.value === tile.id"
                @click="toggle(tile.id, $event.currentTarget)"
              />
            </li>
          </ul>
        </template>
      </section>
    </main>

    <AppDrawer id="connections" :title="t('home.connections')">
      <template #now>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
        <h4>{{ t('twitch.heading') }}</h4>
        <TwitchSignIn :overview="overview" :busy="busy" :run="run" />

        <h4>OBS</h4>
        <p>
          {{ t(obs.key) }}
          <template v-if="live.obs?.reason">— {{ errorMessage(live.obs.reason) }}</template>
        </p>
      </template>
    </AppDrawer>

    <AppDrawer id="chat" :title="moduleName('chat')" full-page="/chat">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('chat')" />
        <form @submit.prevent="simulateChat">
          <p>{{ t('simulate.hint') }}</p>
          <label for="sim-user">{{ t('simulate.userName') }}</label>
          <InputText id="sim-user" v-model="simUser" autocomplete="off" maxlength="25" />
          <label for="sim-text">{{ t('simulate.text') }}</label>
          <InputText id="sim-text" v-model="simText" autocomplete="off" maxlength="500" required />
          <Button type="submit" :label="t('simulate.send')" icon="pi pi-send" :disabled="busy" />
        </form>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
      <template #now>
        <ChatNow v-model:log="chatLog" :broadcaster-id="broadcasterId" :disconnected="live.status === 'failed'" />
      </template>
    </AppDrawer>

    <AppDrawer id="alerts" :title="moduleName('alerts')">
      <template #actions>
        <Button :label="t('chat.copyObsUrl')" icon="pi pi-copy" outlined @click="copyObsUrl('alerts')" />
        <form @submit.prevent="simulateAlert">
          <p>{{ t('simulate.hint') }}</p>
          <span id="sim-kind">{{ t('simulate.kind') }}</span>
          <SelectButton
            v-model="simAlert.kind"
            :options="ALERT_KINDS"
            :option-label="(k: AlertKind) => t(`alerts.kinds.${k}`)"
            :allow-empty="false"
            aria-labelledby="sim-kind"
          />
          <label for="sim-alert-user">{{ t('simulate.userName') }}</label>
          <InputText id="sim-alert-user" v-model="simAlert.userName" autocomplete="off" maxlength="25" />
          <template v-if="simAlert.kind in AMOUNT_MAX">
            <label for="sim-amount">{{ t(`alerts.amount.${simAlert.kind}`) }}</label>
            <InputNumber
              v-model="simAlert.amount"
              input-id="sim-amount"
              :min="1"
              :max="AMOUNT_MAX[simAlert.kind]"
              :use-grouping="false"
              required
            />
          </template>
          <template v-if="simAlert.kind === 'resubscribe' || simAlert.kind === 'cheer'">
            <label for="sim-alert-message">{{ t('simulate.message') }}</label>
            <InputText id="sim-alert-message" v-model="simAlert.message" autocomplete="off" maxlength="500" />
          </template>
          <Button type="submit" :label="t('simulate.sendAlert')" icon="pi pi-send" :disabled="busy" />
        </form>
        <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
      </template>
    </AppDrawer>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import SelectButton from 'primevue/selectbutton'
import { useToast } from 'primevue/usetoast'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { errorMessage } from '../i18n'
import { ALERT_KINDS, type AlertKind, alertTitle } from '../overlay/alerts/alerts'
import { ApiError, type Schemas, api, client } from './api'
import AppDrawer from './AppDrawer.vue'
import ArrangeTiles from './ArrangeTiles.vue'
import ChatNow from './ChatNow.vue'
import { EMPTY_LOG, applyChat } from './chatLog'
import { drawerId, useDrawer } from './drawer'
import {
  type Line,
  arrangedTiles,
  errorToast,
  obsLight,
  successToast,
  tileGroups,
  twitchLight,
  useLiveEvents,
} from './home'
import HomeTile from './HomeTile.vue'
import { focusReturnTarget } from './page'
import StatusLight from './StatusLight.vue'
import TwitchSignIn from './TwitchSignIn.vue'

const { t, te } = useI18n()
const toast = useToast()
const router = useRouter()

const overview = ref<Schemas['Overview']>()
const overviewFailed = ref(false)
const modules = ref<Schemas['ModuleState'][]>()
const loadFailed = ref(false)
const busy = ref(false)
const simUser = ref('')
const simText = ref('')
// The Host's limits for the kinds that take an amount (crates/host/src/admin.rs).
const AMOUNT_MAX: Partial<Record<AlertKind, number>> = { resubscribe: 100000, gift: 100, cheer: 100000, raid: 100000 }
const simAlert = reactive({ kind: 'follow' as AlertKind, userName: '', amount: 5 as number | null, message: '' })
const templateError = ref<string | null>(null)
const notice = ref<{ text: string; error: boolean }>()
const tilesEl = ref<HTMLElement>()
// Saved tile order (`null`: never arranged). Only this page changes it, so it is read once with the modules.
const savedTiles = ref<string[] | null>(null)
const arranging = ref(false)
const arrangeButton = ref<{ $el: HTMLElement }>()
const doneButton = ref<{ $el: HTMLElement }>()
let savingTiles = false
let timer: number | undefined

const drawer = useDrawer()
watch(drawer.current, () => (notice.value = undefined))
// Kept here, not in the drawer, so messages keep arriving while it is closed.
const chatLog = shallowRef(EMPTY_LOG)
const live = useLiveEvents(2000, (msg) => (chatLog.value = applyChat(chatLog.value, msg)))
const broadcasterId = computed(() =>
  overview.value?.status.state === 'signed_in' ? overview.value.status.user_id : undefined,
)
const twitch = computed(() => twitchLight(overview.value, overviewFailed.value))
const obs = computed(() => obsLight(live.value))
const groups = computed(() => tileGroups(modules.value ?? [], arrangedTiles(savedTiles.value)))
const pending = computed<{ id: string; text: string; panel: string }[]>(() => [])

const lines = computed<Record<string, Line>>(() => {
  const { status, lastChat, lastAlert } = live.value
  return {
    chat:
      status === 'open'
        ? { state: 'ready', text: lastChat ? `${lastChat.payload.userName}：${lastChat.payload.text}` : t('home.chatEmpty') }
        : { state: status },
    alerts:
      status === 'open'
        ? { state: 'ready', text: lastAlert ? alertTitle(lastAlert.payload) : t('alerts.empty') }
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
  if (!modules.value) savedTiles.value = await api(client.GET('/api/admin/home-tiles'))
  modules.value = await api(client.GET('/api/admin/modules'))
  await refreshTemplate()
}

async function refreshTemplate() {
  templateError.value = (await api(client.GET('/api/admin/templates/chat'))).error
}

// Toasts sit under a modal drawer's top layer, so results show inside the drawer while it is modal.
function notify(text: string, error: boolean) {
  if (document.querySelector('.drawer-dialog[open]')) notice.value = { text, error }
  else toast.add(error ? errorToast(text) : successToast(text))
}

// Tiles and lights are disclosure buttons: a second press closes their drawer.
const toggle = (id: string, el: EventTarget | null) =>
  drawer.current.value === id ? drawer.close() : drawer.open(id, el)

async function run(action: () => Promise<unknown>) {
  busy.value = true
  notice.value = undefined
  try {
    await action()
    await refresh()
  } catch (e) {
    loadFailed.value = !modules.value
    notify(e instanceof ApiError ? errorMessage(e.code) : String(e), true)
  } finally {
    busy.value = false
  }
}

const simulateChat = () =>
  run(async () => {
    await api(client.POST('/api/admin/simulate/chat', { body: { userName: simUser.value, text: simText.value } }))
    simText.value = ''
    notify(t('simulate.sent'), false)
  })

const simulateAlert = () =>
  run(async () => {
    const { kind, userName, amount, message } = simAlert
    await api(client.POST('/api/admin/simulate/alert', { body: { kind, userName, amount, message } }))
    notify(t('simulate.alertSent'), false)
  })

async function copyObsUrl(overlay: 'chat' | 'alerts') {
  notice.value = undefined
  try {
    // The admin window loads the Host URL, so its origin is what OBS needs.
    // shortcut: under `tauri dev` this is the Vite URL, which works only while Vite runs.
    await navigator.clipboard.writeText(`${location.origin}/overlay/${overlay}/`)
    notify(t('chat.copied'), false)
  } catch {
    notify(t('chat.copyFailed'), true)
  }
}

// The section itself shows no focus ring, so it only takes focus while it has nothing focusable (loading, failed).
const focusTiles = () =>
  (tilesEl.value?.querySelector<HTMLElement>('[data-drawer-tile], [data-drawer-arrange]') ?? tilesEl.value)?.focus()

/** Closes an open drawer first (submitting its changes); a failed submit keeps the drawer and the normal home. */
async function enterArrange() {
  if (!(await drawer.close())) return
  arranging.value = true
  await nextTick()
  doneButton.value?.$el.focus()
}

async function leaveArrange() {
  arranging.value = false
  await nextTick()
  arrangeButton.value?.$el.focus()
}

/**
 * Shows the tile order `next` at once and saves it; a failure puts the previous order back and reports it, even
 * after arrange mode was left. Returns nothing while another save runs, so saves never overlap.
 */
function saveTiles(next: string[] | null): Promise<boolean> | undefined {
  if (savingTiles) return
  savingTiles = true
  const previous = savedTiles.value
  savedTiles.value = next
  return api(client.PUT('/api/admin/home-tiles', { body: next }))
    .then(
      () => true,
      (e) => {
        savedTiles.value = previous
        notify(e instanceof ApiError ? errorMessage(e.code) : String(e), true)
        return false
      },
    )
    .finally(() => (savingTiles = false))
}

// Esc leaves arrange mode, but an open drawer or dialog (the close hint) takes it first. Listens in the capture
// phase: browsers run microtasks between listeners, so the drawer's own Esc listener may have finished closing it
// before a bubbling listener here would run.
function onKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape' || e.defaultPrevented || !arranging.value) return
  if (!drawer.current.value && !document.querySelector('dialog[open]')) leaveArrange()
}

onMounted(async () => {
  document.addEventListener('keydown', onKeydown, true)
  await run(async () => {})
  await focusReturnTarget()
  // Statuses and the template are polled, so a lost sign-in or a broken template edit shows up without a reload.
  // Until the first full load succeeds, retry all of it, so one startup failure does not leave the tiles empty.
  timer = window.setInterval(() => {
    const poll = modules.value ? Promise.all([refreshOverview(), refreshTemplate()]) : refresh()
    poll.catch(() => {})
  }, 2000)
})
onBeforeUnmount(() => {
  window.clearInterval(timer)
  document.removeEventListener('keydown', onKeydown, true)
})
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

/* Wider than 1000px the drawer is an aside beside the page. */
.layout {
  display: flex;
  align-items: flex-start;
}

.layout > main {
  flex: 1;
  min-width: 0;
}

h4 {
  margin: 0;
}
</style>
