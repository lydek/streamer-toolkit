<template>
  <!-- 虛擬形象: VTuber Plus, VSeeFace, the mic avatar and outfits. Drawn ahead of its backend (dev mode only). -->
  <FullPage>
    <div class="avatar">
      <div class="apps">
        <section v-for="a in APPS" :key="a" class="tbl-wrap" :aria-labelledby="`av-${a}`">
          <div class="tbl-top">
            <h3 :id="`av-${a}`" class="tbl-head">{{ t(`avatar.apps.${a}`) }}</h3>
            <Button :label="t('avatar.setup')" severity="secondary" size="small" disabled />
          </div>
          <p class="empty-cell">{{ t('avatar.notConnected') }}</p>
        </section>
      </div>

      <section class="tbl-wrap" aria-labelledby="av-faces">
        <h3 id="av-faces" class="tbl-head">{{ t('avatar.faces') }}</h3>
        <div class="faces">
          <Button v-for="f in FACES" :key="f" :label="t(`avatar.face.${f}`)" severity="secondary" disabled />
        </div>
      </section>

      <section class="tbl-wrap" aria-labelledby="av-items">
        <h3 id="av-items" class="tbl-head">{{ t('avatar.items') }}</h3>
        <p class="empty-cell">{{ t('avatar.noItems') }}</p>
        <div class="tbl-foot">
          <span class="note">{{ t('avatar.itemsNote') }}</span>
          <Button :label="t('avatar.addItem')" icon="pi pi-plus" disabled />
        </div>
      </section>

      <div class="apps">
        <FormPanel :heading="t('avatar.mic.heading')">
          <FormRow :label="t('avatar.mic.device')" input-id="av-mic">
            <Select input-id="av-mic" :options="[]" :placeholder="t('avatar.mic.noDevice')" disabled />
          </FormRow>
          <FormRow :label="t('avatar.mic.level')" label-id="av-level">
            <!-- No mic yet: the bar stays empty; the line marks the speaking threshold. -->
            <span class="meter" role="img" :aria-label="t('avatar.mic.levelNone')">
              <b :style="{ left: `${threshold}%` }"></b>
            </span>
          </FormRow>
          <FormRow :label="t('avatar.mic.threshold')" input-id="av-threshold">
            <span class="range">
              <input id="av-threshold" v-model.number="threshold" type="range" min="0" max="100" />
              <output for="av-threshold">{{ threshold }}%</output>
            </span>
          </FormRow>
          <FormRow :label="t('avatar.mic.highPass')" label-id="av-high-pass" end>
            <ToggleSwitch disabled aria-labelledby="av-high-pass" />
          </FormRow>
          <FormRow :label="t('avatar.mic.smoothing')" input-id="av-smoothing">
            <span class="range">
              <input id="av-smoothing" v-model.number="smoothing" type="range" min="0" max="100" />
              <output for="av-smoothing">{{ smoothing }}%</output>
            </span>
          </FormRow>
          <FormRow :label="t('avatar.mic.afk')" input-id="av-afk">
            <span class="unit-field">
              <InputNumber v-model="afk" input-id="av-afk" :min="0" :max="60" :use-grouping="false" />
              <span>{{ t('avatar.mic.afkUnit') }}</span>
            </span>
          </FormRow>
          <FormRow :label="t('avatar.mic.animation')" input-id="av-animation">
            <Select
              v-model="animation"
              input-id="av-animation"
              :options="ANIMATIONS"
              :option-label="(a: string) => t(`avatar.mic.animations.${a}`)"
            />
          </FormRow>
        </FormPanel>
        <section class="tbl-wrap" aria-labelledby="av-sets">
          <div class="tbl-top">
            <h3 id="av-sets" class="tbl-head">{{ t('avatar.sets') }}</h3>
            <Button :label="t('avatar.addSet')" icon="pi pi-plus" severity="secondary" size="small" disabled />
          </div>
          <p class="empty-cell">{{ t('avatar.noSets') }}</p>
        </section>
      </div>

      <FormPanel :heading="t('avatar.outfits.heading')">
        <FormRow :label="t('avatar.outfits.list')" label-id="av-outfits">
          <span class="muted">{{ t('avatar.outfits.none') }}</span>
        </FormRow>
        <FormRow :label="t('avatar.outfits.reward')" input-id="av-reward">
          <Select input-id="av-reward" :options="[]" :placeholder="t('avatar.outfits.noReward')" disabled />
        </FormRow>
        <FormRow :label="t('avatar.outfits.back')" input-id="av-back">
          <span class="unit-field">
            <InputNumber v-model="back" input-id="av-back" :min="0" :max="120" :use-grouping="false" />
            <span>{{ t('avatar.outfits.backUnit') }}</span>
          </span>
        </FormRow>
      </FormPanel>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import Select from 'primevue/select'
import ToggleSwitch from 'primevue/toggleswitch'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import FormPanel from './FormPanel.vue'
import FormRow from './FormRow.vue'
import FullPage from './FullPage.vue'

const APPS = ['vtuberPlus', 'vseeface']
const FACES = ['joy', 'angry', 'sorrow', 'fun', 'surprised', 'neutral']
const ANIMATIONS = ['bounce', 'fade', 'zoom', 'slide', 'cut']

const { t } = useI18n()
const threshold = ref(30)
const smoothing = ref(60)
const afk = ref(3)
const animation = ref('bounce')
const back = ref(10)
</script>

<style scoped>
.avatar {
  display: grid;
  gap: 1rem;
  max-width: 90rem;
}

.avatar .tbl-wrap {
  margin-top: 0;
}

.apps {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(22rem, 100%), 1fr));
  gap: 1rem;
  align-items: start;
}

.faces {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  padding: 0.75rem;
}

.range,
.unit-field {
  display: inline-flex;
  align-items: center;
  gap: 0.75rem;
}

.meter {
  position: relative;
  display: block;
  width: min(14rem, 100%);
  height: 0.5rem;
  border-radius: 999px;
  background: var(--p-surface-800);
}

.meter b {
  position: absolute;
  top: -0.25rem;
  bottom: -0.25rem;
  width: 2px;
  background: var(--p-text-muted-color);
}

.range input {
  width: min(14rem, 100%);
}

.unit-field :deep(.p-inputnumber-input) {
  width: 5rem;
}

.note,
.muted {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}
</style>
