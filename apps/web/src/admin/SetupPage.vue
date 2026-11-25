<template>
  <!-- 首次設定: five steps on a data folder that never finished or skipped them (setup.ts); Settings → Connections
       can change all of it later. -->
  <FullPage :title="t('setup.title')">
    <div class="setup">
      <ol class="steps" :aria-label="t('setup.steps')">
        <li v-for="(s, i) in STEPS" :key="s" :aria-current="i === step ? 'step' : undefined" :class="{ done: i < step }">
          {{ t(`setup.step.${s}`) }}
        </li>
      </ol>

      <section class="panel" :aria-labelledby="`setup-${STEPS[step]}`">
        <h2 :id="`setup-${STEPS[step]}`" ref="heading" tabindex="-1">{{ t(`setup.heading.${STEPS[step]}`) }}</h2>

        <template v-if="STEPS[step] === 'register'">
          <ol class="guide">
            <li v-for="n in 3" :key="n">{{ t(`setup.register.${n}`) }}</li>
          </ol>
          <p v-for="c in COPY" :key="c.id" class="copy">
            <code>{{ c.text }}</code>
            <Button icon="pi pi-copy" text :aria-label="t(`setup.copy.${c.id}`)" @click="copy(c.text)" />
          </p>
        </template>

        <!-- One TwitchSignIn shows the Client ID form until one is saved, then the sign-in: each step shows its half. -->
        <template v-else-if="STEPS[step] === 'clientId'">
          <p v-if="overview?.client_id" class="muted">{{ t('setup.clientIdSaved') }}</p>
          <TwitchSignIn v-else :overview="overview" :busy="busy" :run="run" />
        </template>
        <template v-else-if="STEPS[step] === 'signIn'">
          <TwitchSignIn v-if="overview?.client_id" :overview="overview" :busy="busy" :run="run" />
          <p v-else class="muted">{{ t('setup.clientIdFirst') }}</p>
        </template>

        <form v-else-if="STEPS[step] === 'obs'" class="obs" @submit.prevent="next">
          <p class="muted">{{ t('setup.obs.where') }}</p>
          <label for="setup-obs-host">{{ t('settings.obs.host') }}</label>
          <InputText id="setup-obs-host" v-model="obs.host" autocomplete="off" />
          <label for="setup-obs-port">{{ t('settings.obs.port') }}</label>
          <InputText id="setup-obs-port" v-model="obs.port" inputmode="numeric" autocomplete="off" />
          <label for="setup-obs-password">{{ t('settings.obs.password') }}</label>
          <InputText id="setup-obs-password" v-model="obs.password" type="password" autocomplete="new-password" />
        </form>

        <template v-else>
          <p class="muted">{{ t('setup.overlays.how') }}</p>
          <p v-for="o in OVERLAY_IDS" :key="o" class="copy">
            <span class="name">{{ t(`obsSources.names.${o}`) }}</span>
            <code>{{ overlayUrl(o) }}</code>
            <Button icon="pi pi-copy" text :aria-label="t('setup.copy.overlay', { name: t(`obsSources.names.${o}`) })" @click="copy(overlayUrl(o))" />
          </p>
        </template>
      </section>

      <div class="nav">
        <Button :label="t('setup.back')" severity="secondary" :disabled="step === 0 || busy" @click="go(step - 1)" />
        <Button :label="t('setup.skip')" text :disabled="busy" @click="finish" />
        <Button :label="step === STEPS.length - 1 ? t('setup.finish') : t('setup.next')" :disabled="busy" @click="next" />
      </div>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import { useToast } from 'primevue/usetoast'
import { nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, successToast } from './home'
import { OVERLAYS } from './obsSources'
import { finishSetup } from './setup'
import TwitchSignIn from './TwitchSignIn.vue'

const STEPS = ['register', 'clientId', 'signIn', 'obs', 'overlays'] as const
// The Redirect URL the Twitch app form asks for, and where to register it (docs/decisions.md Twitch app).
const COPY = [
  { id: 'redirect', text: 'http://localhost' },
  { id: 'console', text: 'https://dev.twitch.tv/console/apps' },
]
const OVERLAY_IDS = ['chat', 'alerts', 'checkin', 'sound']

const { t } = useI18n()
const toast = useToast()
const router = useRouter()
const step = ref(0)
const heading = ref<HTMLElement>()
const busy = ref(false)
const overview = ref<Schemas['Overview']>()
const obs = reactive({ host: '127.0.0.1', port: '4455', password: '' })

const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
const overlayUrl = (id: string) => `${location.origin}${OVERLAYS.find((o) => o.id === id)!.path}`

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    toast.add(successToast(t('setup.copied')))
  } catch {
    toast.add(errorToast(t('chat.copyFailed')))
  }
}

async function refreshTwitch() {
  overview.value = await api(client.GET('/api/admin/twitch'))
}

async function run(action: () => Promise<unknown>) {
  busy.value = true
  try {
    await action()
    await refreshTwitch()
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}

async function go(to: number) {
  step.value = to
  await nextTick()
  heading.value?.focus()
}

// Leaving the OBS step saves the address, the way Settings → Connections does; the Host checks the host name.
async function saveObs() {
  const port = Number(obs.port)
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new ApiError('obs.invalid_address')
  await api(client.PUT('/api/admin/obs', { body: { host: obs.host, port, ...(obs.password ? { password: obs.password } : {}) } }))
}

async function next() {
  if (step.value < STEPS.length - 1) {
    if (STEPS[step.value] === 'obs') {
      busy.value = true
      try {
        await saveObs()
      } catch (e) {
        report(e)
        return
      } finally {
        busy.value = false
      }
    }
    await go(step.value + 1)
  } else await finish()
}

async function finish() {
  busy.value = true
  try {
    await finishSetup()
    await router.push('/')
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}

// Saving the Client ID on its step moves on to signing in.
watch(
  () => overview.value?.client_id,
  (id, before) => {
    if (id && !before && STEPS[step.value] === 'clientId') void go(step.value + 1)
  },
)

// Polled so a sign-in finished in the browser shows up, as in Settings.
let timer = 0
onMounted(async () => {
  timer = window.setInterval(() => void refreshTwitch().catch(() => {}), 2000)
  await refreshTwitch().catch(report)
  const saved = await api(client.GET('/api/admin/obs')).catch(() => undefined)
  if (saved) Object.assign(obs, { host: saved.host, port: String(saved.port) })
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>

<style scoped>
.setup {
  display: grid;
  gap: 1rem;
  max-width: 48rem;
}

.steps {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1.25rem;
  margin: 0;
  padding: 0;
  list-style: none;
  counter-reset: step;
  color: var(--p-text-muted-color);
}

.steps li {
  counter-increment: step;
}

.steps li::before {
  content: counter(step) '. ';
}

.steps li[aria-current='step'] {
  color: var(--p-primary-color);
  font-weight: 600;
}

.steps li.done {
  color: var(--p-text-color);
}

.panel {
  display: grid;
  gap: 0.75rem;
  padding: 1rem 1.25rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.panel h2 {
  margin: 0;
  font-size: 1.25rem;
}

.guide {
  margin: 0;
  padding-left: 1.25rem;
}

.copy {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.25rem 0.75rem;
  margin: 0;
}

.copy code {
  overflow-wrap: anywhere;
}

.copy .name {
  min-width: 4rem;
}

.obs {
  display: grid;
  grid-template-columns: max-content minmax(0, 16rem);
  gap: 0.5rem 1rem;
  align-items: center;
}

.obs .muted {
  grid-column: 1 / -1;
}

.muted {
  margin: 0;
  color: var(--p-text-muted-color);
}

.nav {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.nav > :last-child {
  margin-left: auto;
}
</style>
