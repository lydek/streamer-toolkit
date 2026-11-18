<template>
  <!-- Settings › features: every feature by group (features.ts). Built ones switch their module on and off; the
       rest say 「即將推出」. -->
  <div class="feat-head">
    <InputText v-model="query" type="search" :placeholder="t('features.search')" :aria-label="t('features.search')" />
    <span v-if="modules" role="status">{{ t('features.count', count(switchable)) }}</span>
  </div>
  <p v-if="!modules">{{ loadText }}</p>
  <template v-else>
    <p v-if="!shownGroups.length">{{ t('features.empty') }}</p>
    <section v-for="g in shownGroups" :key="g.id" class="feat-group" :aria-labelledby="`fg-${g.id}`">
      <h3 :id="`fg-${g.id}`">
        <i :class="g.icon" aria-hidden="true"></i>{{ t(`nav.groups.${g.id}`) }}
        <span class="group-count">{{ t('features.groupCount', count(g.all)) }}</span>
      </h3>
      <ul class="feat-list">
        <li v-for="f in g.features" :key="f.id" class="feat" :class="{ off: !on(f) }">
          <i :class="f.icon" class="feat-icon" aria-hidden="true"></i>
          <span class="feat-text">
            <span class="name">{{ name(f) }}</span>
            <span :id="`fd-${f.id}`" class="desc">{{ t(`features.${f.id}.desc`) }}</span>
          </span>
          <span class="tags">
            <span v-if="f.needs" class="tag">{{ t(`features.needs.${f.needs}`) }}</span>
            <span v-if="f.scopes && !on(f)" class="tag scope" :title="f.scopes.join(' ')">{{ t('features.needs.scopes') }}</span>
            <span v-if="!f.module && !f.fixed" class="tag">{{ t('home.soon') }}</span>
          </span>
          <span v-if="f.fixed" class="fixed"><i class="pi pi-lock" aria-hidden="true"></i>{{ t('features.alwaysOn') }}</span>
          <template v-else>
            <RouterLink v-if="on(f)" :to="openPage(f)" class="open">
              {{ t('features.open') }}<span class="sr-only">：{{ name(f) }}</span>
            </RouterLink>
            <ToggleSwitch
              :input-id="f.module ? `module-${f.module}` : undefined"
              :model-value="on(f)"
              :disabled="busy || !f.module"
              :aria-label="name(f)"
              @update:model-value="toggle(f)"
            />
          </template>
        </li>
      </ul>
    </section>
  </template>

  <dialog ref="confirmEl" class="sheet confirm" aria-labelledby="feat-confirm-title" @close="onConfirmClose">
    <h2 id="feat-confirm-title">{{ confirm?.title }}</h2>
    <p>{{ confirm?.text }}</p>
    <ul>
      <li v-for="line in confirm?.lines" :key="line">{{ line }}</li>
    </ul>
    <div class="buttons sheet-actions">
      <Button :label="confirm?.ok" autofocus @click="confirmEl?.close('ok')" />
      <Button :label="t('features.cancel')" text @click="confirmEl?.close()" />
    </div>
  </dialog>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import ToggleSwitch from 'primevue/toggleswitch'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Schemas } from './api'
import { FEATURES, type Feature, hiddenPages, isOn, turnOffPlan, turnOnPlan } from './features'
import { GROUPS, SOON_PAGES } from './tabs'

const props = defineProps<{ modules?: Schemas['ModuleState'][]; busy: boolean; loadText: string }>()
const emit = defineEmits<{ toggle: [module: Schemas['ModuleState']] }>()
const { t } = useI18n()

const query = ref('')
const on = (f: Feature) => isOn(f, props.modules ?? [])
const name = (f: Feature) => t(`features.${f.id}.name`)
const hidden = computed(() => hiddenPages(props.modules ?? []))
const SOON = new Set(SOON_PAGES.map((p) => p.path))
// Like a group tab (firstPage): the first built page that is shown, else the first shown page.
const openPage = (f: Feature) => {
  const shown = f.pages.filter((p) => !hidden.value.has(p))
  return shown.find((p) => !SOON.has(p)) ?? shown[0] ?? f.pages[0]
}
const switchable = FEATURES.filter((f) => !f.fixed)
// Always-on home is not counted: only features that can be switched.
const count = (fs: Feature[]) => {
  const s = fs.filter((f) => !f.fixed)
  return { on: s.filter(on).length, all: s.length }
}

const shownGroups = computed(() => {
  const q = query.value.trim().toLowerCase()
  const matches = (f: Feature) => !q || `${name(f)} ${t(`features.${f.id}.desc`)}`.toLowerCase().includes(q)
  return GROUPS.map((g) => {
    const all = FEATURES.filter((f) => f.group === g.id)
    return { id: g.id, icon: g.icon, all, features: all.filter(matches) }
  }).filter((g) => g.features.length)
})

// One confirmation dialog for what a switch takes along (features.ts plans).
const confirmEl = ref<HTMLDialogElement>()
const confirm = ref<{ title: string; text: string; lines: string[]; ok: string; then: () => void }>()
function ask(c: NonNullable<typeof confirm.value>) {
  confirm.value = c
  confirmEl.value?.showModal()
}
function onConfirmClose() {
  if (confirmEl.value?.returnValue === 'ok') confirm.value?.then()
  if (confirmEl.value) confirmEl.value.returnValue = ''
}

const moduleOf = (f: Feature) => props.modules?.find((m) => m.id === f.module)
const flip = (fs: Feature[]) => fs.forEach((f) => moduleOf(f) && emit('toggle', moduleOf(f)!))

function toggle(f: Feature) {
  if (!f.module) return
  if (!on(f)) {
    // shortcut: scopes are not requested here yet; no built feature adds any. Wire the re-sign-in for
    // `turnOnPlan().scopes` when the first one that does is built (docs/decisions.md Twitch scope).
    const { requires } = turnOnPlan(f, on, new Set())
    if (!requires.length) return flip([f])
    return ask({
      title: t('features.confirm.onTitle', { name: name(f) }),
      text: t('features.confirm.onText'),
      lines: requires.map(name),
      ok: t('features.confirm.onOk'),
      then: () => flip([...requires, f]),
    })
  }
  const { dependents, affected } = turnOffPlan(f, on)
  if (!dependents.length && !affected.length) return flip([f])
  ask({
    title: t('features.confirm.offTitle', { name: name(f) }),
    text: t(dependents.length ? 'features.confirm.offDependents' : 'features.confirm.offAffected'),
    lines: [...dependents.map(name), ...affected.map(() => t(`features.${f.id}.affects`))],
    ok: t('features.confirm.offOk'),
    then: () => flip([...dependents, f]),
  })
}
</script>

<style scoped>
.feat-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
  margin-bottom: 0.75rem;
}

.feat-group h3 {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin: 1rem 0 0.25rem;
  font-size: 1rem;
}

.feat-group h3 i {
  color: var(--p-text-muted-color);
}

.group-count {
  margin-left: auto;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
  font-weight: 400;
}

.feat-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.feat {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 0.75rem;
  padding: 0.625rem 0;
  border-top: 1px solid var(--p-surface-800);
}

.feat-icon {
  width: 1.25rem;
  color: var(--p-text-muted-color);
}

.feat-text {
  display: grid;
  flex: 1 1 12rem;
  min-width: 0;
}

.name {
  font-weight: 600;
}

.desc {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.off .name {
  color: var(--p-text-muted-color);
}

.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.tag {
  padding: 0 0.5rem;
  border-radius: 999px;
  background: var(--p-surface-800);
  color: var(--p-text-muted-color);
  font-size: 0.75rem;
  white-space: nowrap;
}

/* yellow-400 on surface-800: well above 4.5:1. */
.tag.scope {
  color: var(--p-yellow-400);
}

.fixed {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.open {
  white-space: nowrap;
}

.confirm {
  max-width: min(28rem, calc(100vw - 2rem));
  border: 1px solid var(--p-content-border-color);
  border-radius: var(--p-content-border-radius);
  background: var(--p-content-background);
  color: var(--p-text-color);
}

.confirm[open] {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.confirm::backdrop {
  background: rgb(0 0 0 / 0.6);
}

.confirm h2,
.confirm p,
.confirm ul {
  margin: 0;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}
</style>
