<template>
  <!-- Settings › features: every feature by group, under its category (tabs.ts). Built ones switch their module on
       and off; the rest say 「即將推出」. -->
  <div class="feat-head">
    <InputText v-model="query" type="search" :placeholder="t('features.search')" :aria-label="t('features.search')" />
    <span v-if="modules" role="status">{{ t('features.count', count(switchable)) }}</span>
  </div>
  <p v-if="!modules">{{ loadText }}</p>
  <template v-else>
    <!-- Not using Twitch or OBS at all: what needs it goes off and is locked, its tabs and tiles fold away. -->
    <div class="svc-use" role="group" :aria-label="t('settings.features')">
      <div v-for="s in SERVICES" :key="s" class="svc">
        <span class="svc-icon" :class="s === 'twitch' ? 'tint-primary' : 'tint-blue'" aria-hidden="true">
          <i :class="s === 'twitch' ? 'pi pi-twitch' : 'pi pi-desktop'"></i>
        </span>
        <span class="svc-text">
          <label :for="`use-${s}`" class="name">{{ t('services.use', { service: SERVICE_NAMES[s] }) }}</label>
          <span :id="`use-${s}-desc`" class="desc">{{ t('services.useDesc', { service: SERVICE_NAMES[s] }) }}</span>
        </span>
        <ToggleSwitch
          :key="`use-${s}-${drawn}`"
          class="svc-switch"
          :input-id="`use-${s}`"
          :model-value="!services.unused.includes(s)"
          :disabled="busy"
          :aria-describedby="`use-${s}-desc`"
          @update:model-value="(use: boolean) => switchService(s, use)"
        />
      </div>
    </div>
    <p v-if="!shownGroups.length">{{ t('features.empty') }}</p>
    <section v-for="g in shownGroups" :key="g.id" class="feat-group" :aria-labelledby="`fg-${g.id}`">
      <div class="group-head">
        <h3 :id="`fg-${g.id}`" tabindex="-1">
          <i :class="g.icon" aria-hidden="true"></i>{{ t(`nav.groups.${g.id}`) }}
          <span class="group-count">{{ t('features.groupCount', count(g.all)) }}</span>
        </h3>
        <!-- The whole group (not only what a search shows), built and switchable features only. -->
        <span v-if="bulkable(g.all).length > 1" class="bulk">
          <Button
            :label="t('features.bulk.on')"
            :aria-label="t('features.bulk.onLabel', { group: t(`nav.groups.${g.id}`) })"
            size="small"
            text
            :disabled="busy || bulkable(g.all).every(on)"
            @click="bulkOn(g)"
          />
          <Button
            :label="t('features.bulk.off')"
            :aria-label="t('features.bulk.offLabel', { group: t(`nav.groups.${g.id}`) })"
            size="small"
            text
            :disabled="busy || !bulkable(g.all).some(on)"
            @click="bulkOff(g)"
          />
        </span>
      </div>
      <template v-for="s in g.sections" :key="s.id ?? ''">
        <h4 v-if="s.id" class="feat-cat">{{ t(`nav.categories.${s.id}`) }}</h4>
        <ul class="feat-list">
          <li v-for="f in s.features" :key="f.id" class="feat" :class="{ off: !on(f) }">
            <span class="feat-icon" :class="`tint-${tintOf(f)}`" aria-hidden="true"><i :class="f.icon"></i></span>
            <span class="feat-text">
              <span class="name">{{ name(f) }}</span>
              <span :id="`fd-${f.id}`" class="desc">{{ t(`features.${f.id}.desc`) }}</span>
            </span>
            <span class="tags">
              <span v-for="s in blockedBy(f)" :key="`u-${s}`" class="tag unused">{{ t('services.unused', { service: SERVICE_NAMES[s] }) }}</span>
              <span v-if="f.needs" class="tag">{{ t(`features.needs.${f.needs}`) }}</span>
              <span v-for="s in servicesOf(f.id).filter((s) => servicesDown[s] && !services.unused.includes(s))" :key="s" class="tag down">
                {{ t('services.down', { service: SERVICE_NAMES[s] }) }}
              </span>
              <span v-if="f.scopes && !on(f)" class="tag scope" :title="f.scopes.join(' ')">{{ t('features.needs.scopes') }}</span>
              <span v-if="!f.module && !f.fixed" class="tag">{{ t('home.unbuilt') }}</span>
            </span>
            <RouterLink v-if="f.rules && on(f)" :to="f.rules" class="open">
              {{ t('features.rules') }}<span class="sr-only">：{{ name(f) }}</span>
            </RouterLink>
            <span v-if="f.fixed" class="fixed"><i class="pi pi-lock" aria-hidden="true"></i>{{ t('features.alwaysOn') }}</span>
            <template v-else>
              <RouterLink v-if="on(f) || (!f.module && showUnbuilt() && !blockedBy(f).length)" :to="openPage(f)" class="open">
                {{ t('features.open') }}<span class="sr-only">：{{ name(f) }}</span>
              </RouterLink>
              <ToggleSwitch
                :key="`${f.id}-${drawn}`"
                :input-id="f.module ? `module-${f.module}` : undefined"
                :model-value="on(f)"
                :disabled="busy || !f.module || blockedBy(f).length > 0"
                :aria-label="name(f)"
                @update:model-value="toggle(f)"
              />
            </template>
            <!-- What belongs to the switch itself (check-in's held redemptions), under the row. -->
            <div v-if="withSettings?.includes(f.id)" class="feat-settings">
              <slot name="settings" :id="f.id" />
            </div>
          </li>
        </ul>
      </template>
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
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Schemas } from './api'
import { FEATURES, type Feature, type Service, hiddenPages, isOn, servicesOf, turnOffPlan, turnOnPlan, unusedBy } from './features'
import { TILES, servicesDown } from './home'
import { missingScopes } from './scopes'
import { useServices } from './services'
import { GROUPS, SOON_PAGES, showUnbuilt } from './tabs'

const props = defineProps<{ modules?: Schemas['ModuleState'][]; busy: boolean; loadText: string; withSettings?: string[] }>()
/** `bulk`: a whole group's changes, confirmed once; the page applies them (and asks check-in's pause or close).
 * `service`: Twitch or OBS used or not, with what goes off for it (confirmed); the page saves and applies it. */
const emit = defineEmits<{
  toggle: [module: Schemas['ModuleState']]
  bulk: [changes: { id: string; enabled: boolean }[]]
  service: [service: Service, use: boolean, off: string[]]
  /** Turned on with Twitch scopes it still needs: the page signs in again asking for them and applies `changes`
   * once they are granted. */
  scopes: [scopes: string[], changes: { id: string; enabled: boolean }[]]
}>()
const { t } = useI18n()

const query = ref('')
const on = (f: Feature) => isOn(f, props.modules ?? [])
const name = (f: Feature) => t(`features.${f.id}.name`)
const hidden = computed(() => hiddenPages(props.modules ?? [], FEATURES, services.unused))
const SOON = new Set(SOON_PAGES.map((p) => p.path))
// Like a group tab (firstPage): the first built page that is shown, else the first shown page.
const openPage = (f: Feature) => {
  const shown = f.pages.filter((p) => !hidden.value.has(p))
  return shown.find((p) => !SOON.has(p)) ?? shown[0] ?? f.pages[0]
}
const switchable = FEATURES.filter((f) => !f.fixed && !f.openWith)
// Always-on home is not counted: only features that can be switched.
const count = (fs: Feature[]) => {
  const s = fs.filter((f) => !f.fixed)
  return { on: s.filter(on).length, all: s.length }
}

const shownGroups = computed(() => {
  const q = query.value.trim().toLowerCase()
  const matches = (f: Feature) => !q || `${name(f)} ${t(`features.${f.id}.desc`)}`.toLowerCase().includes(q)
  return GROUPS.map((g) => {
    const all = FEATURES.filter((f) => f.group === g.id && !f.openWith)
    const shown = all.filter(matches)
    // A feature sits under the category of its first page; a group without categories lists its features as one.
    const category = (f: Feature) => g.categories?.find((c) => c.pages.some((p) => p.path === f.pages[0]))
    const sections = [
      ...(g.categories ?? []).map((c) => ({ id: c.id as string | undefined, features: shown.filter((f) => category(f) === c) })),
      { id: undefined, features: shown.filter((f) => !category(f)) },
    ].filter((s) => s.features.length)
    return { id: g.id, icon: g.icon, all, sections }
  }).filter((g) => g.sections.length)
})

// One confirmation dialog for what a switch takes along (features.ts plans).
const confirmEl = ref<HTMLDialogElement>()
const confirm = ref<{ title: string; text: string; lines: string[]; ok: string; then: () => void }>()
function ask(c: NonNullable<typeof confirm.value>) {
  confirm.value = c
  // Esc keeps the last return value, so a new question starts without one.
  if (confirmEl.value) confirmEl.value.returnValue = ''
  confirmEl.value?.showModal()
}
function onConfirmClose() {
  if (confirmEl.value?.returnValue === 'ok') confirm.value?.then()
  else {
    lastSwitched.value = undefined
    drawn.value++
  }
  if (confirmEl.value) confirmEl.value.returnValue = ''
}
// A switch keeps the side it was flipped to until its value changes: after a cancel or a save (which may have
// failed and kept the value), the switches are drawn again from what is saved.
const drawn = ref(0)

const moduleOf = (f: Feature) => props.modules?.find((m) => m.id === f.module)
// A switch is disabled while its change saves, which drops its focus; it gets it back once the save is done.
const lastSwitched = ref<string>()
watch(
  () => props.busy,
  (busy) => {
    if (!busy) drawn.value++
    if (busy || !lastSwitched.value) return
    const id = lastSwitched.value
    lastSwitched.value = undefined
    void nextTick(() => document.getElementById(`module-${id}`)?.focus())
  },
)
const flip = (fs: Feature[]) => fs.forEach((f) => moduleOf(f) && emit('toggle', moduleOf(f)!))

const SERVICES: Service[] = ['twitch', 'obs']
const SERVICE_NAMES: Record<Service, string> = { twitch: 'Twitch', obs: 'OBS' }
const services = useServices()
// Unused services a feature needs (itself or through what it requires): it is locked off.
const blockedBy = (f: Feature) => unusedBy(f, services.unused)

/** Not using a service lists the built features on that need it (confirmed); using it again restores them. */
function switchService(s: Service, use: boolean) {
  if (use) return emit('service', s, true, [])
  const off = FEATURES.filter((f) => f.module && on(f) && unusedBy(f, [s]).length)
  const service = SERVICE_NAMES[s]
  ask({
    title: t('services.offTitle', { service }),
    text: t(off.length ? 'services.offText' : 'services.offNone'),
    lines: off.map(name),
    ok: t('services.offOk'),
    then: () => emit('service', s, false, off.map((f) => f.module!)),
  })
}

// A group's built features that have a switch (and no unused service locking them).
const bulkable = (fs: Feature[]) => fs.filter((f) => f.module && !f.fixed && !blockedBy(f).length)
const unique = (fs: Feature[]) => [...new Map(fs.map((f) => [f.id, f])).values()]
const changes = (fs: Feature[], enabled: boolean) => fs.map((f) => ({ id: f.module!, enabled }))
// Twitch scopes a feature, or what it needs turned on with it, still waits for.
const scopesFor = (f: Feature) => [...new Set([...turnOnPlan(f, on, new Set()).requires, f].flatMap(missingScopes))]

/** Everything off in the group, and what it needs elsewhere, after one confirmation listing it all. */
function bulkOn(g: { id: string; all: Feature[] }) {
  // Features still needing Twitch scopes are left off and listed: each asks for its sign-in on its own switch.
  const off = bulkable(g.all).filter((f) => !on(f))
  const grant = off.filter((f) => scopesFor(f).length)
  const targets = off.filter((f) => !grant.includes(f))
  const outside = unique(targets.flatMap((f) => turnOnPlan(f, on, new Set()).requires)).filter((d) => !targets.includes(d))
  const group = t(`nav.groups.${g.id}`)
  ask({
    title: t('features.bulk.onTitle', { group }),
    text: t('features.bulk.onText'),
    lines: [...[...outside, ...targets].map(name), ...grant.map((f) => t('features.scopes.skipped', { name: name(f) }))],
    ok: t('features.bulk.on'),
    then: () => emit('bulk', changes([...outside, ...targets], true)),
  })
}

/** Everything on in the group, and what needs it elsewhere, after one confirmation listing it all. */
function bulkOff(g: { id: string; all: Feature[] }) {
  const targets = bulkable(g.all).filter(on)
  const outside = unique(targets.flatMap((f) => turnOffPlan(f, on).dependents)).filter((d) => !targets.includes(d) && d.module)
  const group = t(`nav.groups.${g.id}`)
  ask({
    title: t('features.bulk.offTitle', { group }),
    text: t('features.bulk.offText'),
    lines: [...targets, ...outside].map(name),
    ok: t('features.bulk.off'),
    then: () => emit('bulk', changes([...outside, ...targets], false)),
  })
}

function toggle(f: Feature) {
  if (!f.module) return
  lastSwitched.value = f.module
  if (!on(f)) {
    const { requires } = turnOnPlan(f, on, new Set())
    // Scopes it still needs: say which, then sign in again for them (docs/decisions.md Twitch scope).
    // It turns on only once they are granted.
    const scopes = scopesFor(f)
    if (scopes.length)
      return ask({
        title: t('features.scopes.title', { name: name(f) }),
        text: t('features.scopes.text'),
        lines: [...requires.map(name), ...scopes],
        ok: t('features.scopes.ok'),
        then: () => emit('scopes', scopes, changes([...requires, f].filter((x) => x.module), true)),
      })
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

/** A feature's colour: its home tile's (by module, or a tile named after its page), primary for one without a tile. */
const tintOf = (f: Feature) =>
  TILES.find((tile) => (f.module && (tile.module ?? tile.id) === f.module) || `/${tile.id}` === f.pages[0])?.tint ?? 'primary'
</script>

<style scoped>
.feat-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem 1rem;
  margin-bottom: 0.75rem;
}

.group-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.25rem 0.75rem;
  margin: 1rem 0 0.25rem;
  scroll-margin-top: calc(var(--appbar-h, 0px) + var(--page-top-h, 0px) + 0.5rem);
}

.bulk {
  display: inline-flex;
  gap: 0.25rem;
}

.feat-group h3 {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 0.5rem;
  margin: 0;
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

.feat-settings {
  flex-basis: 100%;
  padding: 0.5rem 0 0.5rem 2rem;
}

.feat-cat {
  margin: 0.75rem 0 0;
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

/* The prototype's tinted square per feature, in its home tile's colour. */
.feat-icon {
  display: inline-grid;
  flex: none;
  place-items: center;
  width: 2rem;
  height: 2rem;
  border-radius: 6px;
}

.feat.off .feat-icon {
  opacity: 0.6;
}

/* Each group is a card, as the prototype. */
.feat-group {
  margin-top: 1rem;
  padding: 0.25rem 1.125rem 0.5rem;
  border: 1px solid var(--p-surface-500);
  border-radius: 6px;
  background: var(--p-surface-900);
}

.feat-group .group-head {
  margin-top: 0.5rem;
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
.svc-use {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(18rem, 100%), 1fr));
  gap: 0.75rem 1.5rem;
  margin-bottom: 0.75rem;
  padding: 0.875rem 1rem;
  border-radius: 6px;
  background: var(--p-surface-900);
}

.svc {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  min-width: 0;
}

.svc-icon {
  display: grid;
  place-items: center;
  width: 2rem;
  height: 2rem;
  border-radius: 6px;
  flex-shrink: 0;
}

.svc-text {
  display: grid;
  flex: 1 1 0;
  min-width: 0;
}

.svc-switch {
  flex-shrink: 0;
}

.svc-text .desc {
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.tag.unused {
  background: var(--p-surface-700);
}

.tag.down {
  border-color: var(--p-red-400);
  color: var(--p-red-300);
}

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
