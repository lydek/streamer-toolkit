<template>
  <!-- 聊天機器人 › 自動感謝與歡迎: one message per event, each switched on its own; saved as it changes. -->
  <FullPage :status="t('bot.summary')">
    <p v-if="loadFailed" role="alert">{{ loadFailed }}</p>
    <FormPanel v-if="g" class="greetings">
      <FormRow
        v-for="row in GREETINGS"
        :key="row"
        :label="name(row)"
        :input-id="row === 'bits' ? undefined : `greet-${row}`"
        :label-id="row === 'bits' ? 'greet-bits' : undefined"
        top
      >
        <div class="greet">
          <div class="fields">
            <div v-if="row === 'bits'" class="tiers" role="group" aria-labelledby="greet-bits">
              <label v-for="tier in TIERS" :key="tier" class="tier">
                <span>{{ t(`bot.greetings.bits.tiers.${tier}`) }}</span>
                <InputText v-model="g.bits[TIER_KEYS[tier]]" :maxlength="MESSAGE_MAX" :placeholder="t(`bot.greetings.bits.examples.${tier}`)" @change="save" />
              </label>
            </div>
            <InputText
              v-else
              :id="`greet-${row}`"
              v-model="message(row).message"
              :maxlength="MESSAGE_MAX"
              :placeholder="t(`bot.greetings.${row}.example`)"
              @change="save"
            />
            <template v-if="row === 'returning'">
              <SelectButton
                :model-value="g.returning.post ? 'say' : 'mark'"
                :options="RETURNING"
                :option-label="(m: string) => t(`bot.greetings.returning.modes.${m}`)"
                :allow-empty="false"
                :aria-label="t('bot.greetings.returning.how')"
                @update:model-value="(m: string) => { g!.returning.post = m === 'say'; save() }"
              />
              <label class="days">
                {{ t('bot.greetings.returning.after') }}
                <InputNumber
                  v-model="g.returning.days"
                  :min="1"
                  :max="365"
                  :use-grouping="false"
                  input-class="days-input"
                  :aria-label="t('bot.greetings.returning.after')"
                  @blur="save"
                />
                {{ t('bot.greetings.returning.days') }}
              </label>
            </template>
          </div>
          <ToggleSwitch
            :model-value="switchOf(row).enabled"
            :aria-label="t('bot.greetings.enabled', { name: name(row) })"
            @update:model-value="(on: boolean) => { switchOf(row).enabled = on; save() }"
          />
        </div>
        <template v-if="row === 'returning'" #hint>{{ t('bot.greetings.returning.hint') }}</template>
      </FormRow>
      <template #foot>
        <span class="note">{{ t('bot.greetings.note') }}</span>
      </template>
    </FormPanel>
  </FullPage>
</template>

<script setup lang="ts">
import InputNumber from 'primevue/inputnumber'
import InputText from 'primevue/inputtext'
import SelectButton from 'primevue/selectbutton'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'
import { errorToast } from './home'

type Greetings = Schemas['BotGreetings']
type Row = (typeof GREETINGS)[number]
const GREETINGS = ['follow', 'firstChat', 'returning', 'sub', 'bits', 'raid'] as const
const TIERS = ['small', 'mid', 'big'] as const
const TIER_KEYS = { small: 'small', mid: 'medium', big: 'large' } as const
const RETURNING = ['mark', 'say']
// The Host's limit, checked again there.
const MESSAGE_MAX = 500

const { t } = useI18n()
const toast = useToast()
const g = ref<Greetings>()
const loadFailed = ref('')

const name = (row: string) => t(`bot.greetings.${row}.name`)
const key = (row: Row) => (row === 'firstChat' ? 'first_chat' : row)
/** The row's message and switch (bits has three messages, under the same switch). */
const message = (row: Exclude<Row, 'bits'>) => g.value![key(row) as Exclude<keyof Greetings, 'bits'>]
const switchOf = (row: Row) => g.value![key(row)]

async function load() {
  g.value = await api(client.GET('/api/admin/bot/greetings'))
}

/** Saves every greeting; a refusal says why and puts back what is saved. */
async function save() {
  if (!g.value) return
  try {
    g.value = await api(client.PUT('/api/admin/bot/greetings', { body: g.value }))
  } catch (e) {
    toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
    await load().catch(() => {})
  }
}

load().catch((e: unknown) => (loadFailed.value = e instanceof ApiError ? errorMessage(e.code, e.params) : String(e)))
</script>

<style scoped>
.greetings {
  max-width: 60rem;
}

.greet {
  display: flex;
  align-items: flex-start;
  gap: 1rem;
}

.fields {
  display: grid;
  flex: 1;
  gap: 0.5rem;
}

.fields :deep(.p-inputtext) {
  width: 100%;
}

.tiers {
  display: grid;
  gap: 0.5rem;
}

.tier {
  display: grid;
  grid-template-columns: 6rem minmax(0, 1fr);
  align-items: center;
  gap: 0.5rem;
}

.days {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.days :deep(.days-input) {
  width: 4.5rem;
}

.note {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
