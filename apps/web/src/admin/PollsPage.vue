<template>
  <!-- 投票與預測: Twitch's own polls, started and ended here; the live result arrives as `twitch.contest`. -->
  <FullPage :status="t('polls.summary')">
    <div class="polls">
      <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
      <div class="current">
        <section class="tbl-wrap" aria-labelledby="polls-poll">
          <div class="tbl-top">
            <div>
              <h3 id="polls-poll" class="tbl-head">{{ t('polls.kinds.poll') }}</h3>
              <span v-if="poll" class="sub" data-poll-state>{{ pollState }}</span>
            </div>
            <Button v-if="poll?.status === 'active'" :label="t('polls.end')" severity="secondary" :disabled="busy" @click="endPoll" />
          </div>
          <div v-if="poll" class="result">
            <b>{{ poll.title }}</b>
            <div v-for="c in poll.choices" :key="c.id" class="bar" :data-choice="c.id">
              <span class="label">{{ c.title }}</span>
              <span class="track" aria-hidden="true"><i :style="{ width: `${percent(c.votes)}%` }"></i></span>
              <b>{{ percent(c.votes) }}%</b>
            </div>
            <span class="sub">{{ t('polls.voters', { n: totalVotes }) }}</span>
          </div>
          <p v-else class="empty-cell">{{ t('polls.none.poll') }}</p>
        </section>
        <section class="tbl-wrap" aria-labelledby="polls-prediction">
          <h3 id="polls-prediction" class="tbl-head">{{ t('polls.kinds.prediction') }}</h3>
          <p class="empty-cell">{{ t('polls.none.prediction') }}</p>
        </section>
      </div>

      <FormPanel :heading="t('polls.new')">
        <FormRow :label="t('polls.question')" input-id="polls-question">
          <InputText id="polls-question" v-model="title" class="long" :maxlength="TITLE_MAX" :placeholder="t('polls.questionHint')" />
        </FormRow>
        <FormRow :label="t('polls.options')" label-id="polls-options" top>
          <div class="options" role="group" aria-labelledby="polls-options">
            <span v-for="(_, i) in choices" :key="i" class="option">
              <InputText
                v-model="choices[i]"
                :maxlength="CHOICE_MAX"
                :placeholder="t('polls.option', { n: i + 1 })"
                :aria-label="t('polls.option', { n: i + 1 })"
              />
              <Button
                v-if="choices.length > MIN_CHOICES"
                icon="pi pi-times"
                text
                :aria-label="t('polls.removeOption', { n: i + 1 })"
                @click="choices.splice(i, 1)"
              />
            </span>
            <Button :label="t('polls.addOption')" icon="pi pi-plus" text :disabled="choices.length >= MAX_CHOICES" @click="choices.push('')" />
          </div>
        </FormRow>
        <FormRow :label="t('polls.duration')" input-id="polls-duration">
          <Select v-model="seconds" input-id="polls-duration" :options="durations" option-label="label" option-value="value" />
        </FormRow>
        <template #foot>
          <Button :label="t('polls.save')" icon="pi pi-bookmark" text :disabled="!ready || busy" @click="save" />
          <Button :label="t('polls.start')" :disabled="!ready || busy || poll?.status === 'active'" @click="start(draft())" />
        </template>
      </FormPanel>

      <section class="tbl-wrap" aria-labelledby="polls-saved">
        <h3 id="polls-saved" class="tbl-head">{{ t('polls.saved') }}</h3>
        <p v-if="!templates.length" class="empty-cell">{{ t('polls.noSaved') }}</p>
        <ul v-else class="saved">
          <li v-for="(tpl, i) in templates" :key="i">
            <button type="button" class="link" :title="t('polls.fill')" @click="fill(tpl)">{{ tpl.title }}</button>
            <span class="sub">{{ tpl.choices.join(' / ') }} · {{ duration(tpl.seconds) }}</span>
            <Button :label="t('polls.start')" size="small" outlined :disabled="busy || poll?.status === 'active'" @click="start(tpl)" />
            <Button icon="pi pi-trash" size="small" text :aria-label="t('polls.deleteSaved', { name: tpl.title })" :disabled="busy" @click="remove(i)" />
          </li>
        </ul>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { useToast } from 'primevue/usetoast'
import { computed, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import type { Incoming } from '../overlay/chat/chat'
import { ApiError, type Schemas, api, client } from './api'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import { clockText, errorToast, successToast, useLiveEvents } from './home'

type Template = Schemas['PollTemplate']
// Twitch's limits, checked again by the Host.
const TITLE_MAX = 60
const CHOICE_MAX = 25
const MIN_CHOICES = 2
const MAX_CHOICES = 5
const DURATIONS = [60, 120, 180, 300, 600]

const { t } = useI18n()
const toast = useToast()
const contest = ref<Schemas['ContestPayload']>({ poll: null })
const poll = computed(() => contest.value.poll)
const templates = ref<Template[]>([])
const loadFailed = ref('')
const busy = ref(false)
const title = ref('')
const choices = ref(['', ''])
const seconds = ref(120)
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 1000)
onBeforeUnmount(() => clearInterval(ticker))

const message = (e: unknown) => (e instanceof ApiError ? errorMessage(e.code, e.params) : String(e))
const duration = (s: number) => (s % 60 ? t('polls.seconds', { n: s }) : t('polls.minutes', { n: s / 60 }))
const durations = computed(() => DURATIONS.map((value) => ({ value, label: duration(value) })))
const totalVotes = computed(() => poll.value?.choices.reduce((sum, c) => sum + c.votes, 0) ?? 0)
const percent = (votes: number) => (totalVotes.value ? Math.round((votes / totalVotes.value) * 100) : 0)
const pollState = computed(() => {
  const p = poll.value
  if (!p) return ''
  if (p.status !== 'active') return t(`polls.status.${p.status}`)
  return t('polls.left', { time: clockText(Math.max(0, Date.parse(p.endsAt) - now.value)) })
})
const ready = computed(() => !!title.value.trim() && choices.value.filter((c) => c.trim()).length >= MIN_CHOICES)

// The live result: the latest `twitch.contest`, in the snapshot after every (re)connect too.
useLiveEvents(2000, (msg: Incoming) => {
  if (msg.type === 'snapshot') {
    const last = msg.payload['twitch.contest'].at(-1)
    if (last) contest.value = last.payload
  } else if (msg.type === 'twitch.contest') {
    contest.value = msg.payload
  }
})

const draft = (): Template => ({
  title: title.value.trim(),
  choices: choices.value.map((c) => c.trim()).filter(Boolean),
  seconds: seconds.value,
})

function fill(tpl: Template) {
  title.value = tpl.title
  choices.value = [...tpl.choices]
  seconds.value = tpl.seconds
}

async function run(action: () => Promise<void>) {
  busy.value = true
  try {
    await action()
  } catch (e) {
    toast.add(errorToast(message(e)))
  } finally {
    busy.value = false
  }
}

const start = (tpl: Template) =>
  run(async () => {
    contest.value = await api(client.POST('/api/admin/polls', { body: tpl }))
    toast.add(successToast(t('polls.started')))
  })

const endPoll = () =>
  run(async () => {
    const id = poll.value!.id
    contest.value = await api(client.POST('/api/admin/polls/{id}/end', { params: { path: { id } } }))
  })

const saveTemplates = (next: Template[]) =>
  run(async () => {
    templates.value = await api(client.PUT('/api/admin/polls/templates', { body: next }))
  })
const save = () => saveTemplates([...templates.value, draft()])
const remove = (i: number) => saveTemplates(templates.value.filter((_, n) => n !== i))

api(client.GET('/api/admin/polls')).then(
  (c) => (contest.value = c),
  (e: unknown) => (loadFailed.value = message(e)),
)
void api(client.GET('/api/admin/polls/templates')).then((list) => (templates.value = list), () => {})
</script>

<style scoped>
.polls {
  display: grid;
  gap: 1rem;
  max-width: 72rem;
}

.current {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(18rem, 100%), 1fr));
  gap: 1rem;
}

.current .tbl-wrap {
  margin-top: 0;
}

.result {
  display: grid;
  gap: 0.5rem;
  padding: 0.75rem 1rem 1rem;
}

.bar {
  display: grid;
  grid-template-columns: minmax(4rem, 10rem) minmax(0, 1fr) 3rem;
  gap: 0.75rem;
  align-items: center;
}

.bar .label {
  overflow-wrap: anywhere;
}

.track {
  height: 0.5rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  overflow: hidden;
}

.track i {
  display: block;
  height: 100%;
  background: var(--p-primary-color);
}

.bar b {
  text-align: end;
}

.sub {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.long,
.option > .p-inputtext {
  width: 20rem;
  max-width: 100%;
}

.options {
  display: grid;
  gap: 0.5rem;
  justify-items: start;
}

.option {
  display: inline-flex;
  gap: 0.25rem;
  align-items: center;
}

.saved {
  margin: 0;
  padding: 0.25rem 1rem 0.75rem;
  list-style: none;
}

.saved li {
  display: flex;
  flex-wrap: wrap;
  gap: 0.375rem 0.75rem;
  align-items: center;
  padding: 0.5rem 0;
}

.saved li + li {
  border-top: 1px solid var(--p-surface-800);
}

.link {
  padding: 0;
  border: 0;
  background: none;
  color: var(--p-primary-color);
  font: inherit;
  cursor: pointer;
}
</style>
