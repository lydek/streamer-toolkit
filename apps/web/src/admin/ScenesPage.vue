<template>
  <FullPage :title="t('modules.names.scene')" tile="scene" :status="state?.scene ? t('scene.current', { scene: state.scene }) : undefined">
    <p v-if="live.status === 'loading'">{{ t('app.loading') }}</p>
    <!-- OBS down: the page's notice (FullPage) says so; nothing more here. -->
    <template v-else-if="!obsReady(state) && servicesDown.obs" />
    <p v-else-if="!obsReady(state)" role="status">
      {{ t('scene.notConnected') }}
      <RouterLink to="/settings/connections#obs">{{ t('scene.connect') }}</RouterLink>
    </p>
    <!-- Two panes that scroll on their own (wide and tall enough): scenes on the left, the edited scene's items on the
         right, so a long list on either side never pushes the other out of reach. -->
    <div v-else class="scene-panes">
      <section class="stack pane" aria-labelledby="scene-list">
        <h2 id="scene-list">{{ t('scene.scenes') }}</h2>
        <div class="studio">
          <span class="switch">
            <ToggleSwitch
              input-id="studio-mode"
              :model-value="state.studioMode"
              :disabled="studioBusy"
              @update:model-value="(v: boolean) => studio(() => setStudioMode(v))"
            />
            <label for="studio-mode">{{ t('scene.studio') }}</label>
          </span>
          <template v-if="state.studioMode">
            <Select
              v-model="transition"
              :options="state.transitions ?? []"
              :aria-label="t('scene.transitionKind')"
              :disabled="studioBusy"
            />
            <Button
              :label="t('scene.transition')"
              icon="pi pi-step-forward"
              :disabled="studioBusy || !state.preview"
              @click="studio(() => transitionNow(transition ?? undefined))"
            />
          </template>
        </div>
        <p v-if="state.studioMode" class="muted">{{ t('scene.studioNote') }}</p>
        <SceneList :state="state" @error="report" />
      </section>

      <!-- In studio mode everything here edits the preview scene; otherwise the scene on air. -->
      <section v-if="current" class="stack pane" aria-labelledby="scene-items">
        <div class="items-head">
          <h2 id="scene-items">{{ t('scene.items', { scene: current.name }) }}</h2>
          <span class="stag" :class="editingPreview ? 'preview' : 'live'">{{ t(editingPreview ? 'scene.preview' : 'scene.onAir') }}</span>
          <div class="add">
            <Button
              :label="t('scene.addOverlay')"
              icon="pi pi-plus"
              size="small"
              text
              :disabled="!obsSources?.local || !addable.length || adding"
              aria-haspopup="true"
              aria-controls="add-overlay"
              @click="addMenu?.toggle($event)"
            />
            <Menu id="add-overlay" ref="addMenu" :model="addable" popup />
            <span v-if="obsSources && !obsSources.local" class="muted">{{ t('obsSources.notLocal') }}</span>
          </div>
        </div>
        <div class="items-scroll">
        <p v-if="!current.items.length">{{ t('scene.noItems') }}</p>
        <!-- One row per item; its name opens an editor row under it (one at a time), whose tabs group what can be
             changed. Field changes update in place, so the next click lands where it was aimed. -->
        <table v-else class="table">
          <caption>{{ t('scene.order') }}</caption>
          <thead>
            <tr>
              <th scope="col">{{ t('scene.item') }}</th>
              <th scope="col">{{ t('scene.kind') }}</th>
              <th scope="col">{{ t('scene.position') }}</th>
              <th scope="col">{{ t('scene.shown') }}</th>
              <th scope="col">{{ t('scene.locked') }}</th>
              <th scope="col">{{ t('scene.move') }}</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="(item, i) in current.items" :key="`${item.id}:${resets}`">
              <tr
                :class="{ open: open === item.id, moved: moved === item.id, dragging: dragged === item.id, [`drop-${dropSide}`]: over === item.id }"
                :data-item="item.id"
                draggable="true"
                @dragstart="onDragstart($event, item)"
                @dragover="onDragover($event, item)"
                @dragleave="over = null"
                @drop="onDrop($event, item)"
                @dragend="dragged = over = null"
              >
                <th scope="row">
                  <button
                    type="button"
                    class="item-name"
                    :aria-expanded="open === item.id"
                    :aria-controls="`editor-${item.id}`"
                    @click="open = open === item.id ? null : item.id"
                  >
                    <i class="pi pi-angle-right" aria-hidden="true"></i>{{ item.source }}
                  </button>
                  <RouterLink v-if="duplicates.has(item.source)" to="/obs-sources" class="dup">{{ t('scene.duplicate') }}</RouterLink>
                </th>
                <td>{{ kindText(item.kind) }}</td>
                <td class="geo">{{ geoText(item) }}</td>
                <td>
                  <ToggleSwitch
                    :model-value="item.enabled"
                    :disabled="busy.has(item.id)"
                    :aria-label="t('scene.showItem', { item: item.source })"
                    @update:model-value="(v: boolean) => change(item, { enabled: v })"
                  />
                </td>
                <td>
                  <ToggleSwitch
                    :model-value="item.locked"
                    :disabled="busy.has(item.id)"
                    :aria-label="t('scene.lockItem', { item: item.source })"
                    @update:model-value="(v: boolean) => change(item, { locked: v })"
                  />
                </td>
                <td class="move">
                  <Button
                    icon="pi pi-arrow-up"
                    text
                    rounded
                    :disabled="i === 0 || busy.has(item.id)"
                    :aria-label="t('scene.forward', { item: item.source })"
                    @click="move(item, i - 1)"
                  />
                  <Button
                    icon="pi pi-arrow-down"
                    text
                    rounded
                    :disabled="i === current.items.length - 1 || busy.has(item.id)"
                    :aria-label="t('scene.backward', { item: item.source })"
                    @click="move(item, i + 1)"
                  />
                  <Button
                    icon="pi pi-ellipsis-v"
                    text
                    rounded
                    :disabled="busy.has(item.id)"
                    :aria-label="t('scene.moreMoves', { item: item.source })"
                    aria-haspopup="true"
                    aria-controls="move-menu"
                    @click="openMoveMenu($event, item, i)"
                  />
                </td>
              </tr>
              <tr v-if="open === item.id" :id="`editor-${item.id}`" class="editor">
                <td colspan="6">
                  <!-- Tabs only when there is more than one thing to edit. -->
                  <nav v-if="tabsOf(item).length > 1" class="editor-tabs" :aria-label="t('scene.editItem', { item: item.source })">
                    <button
                      v-for="tab in tabsOf(item)"
                      :key="tab"
                      type="button"
                      :aria-current="shownTab(item) === tab ? 'page' : undefined"
                      @click="editorTab = tab"
                    >
                      {{ t(`scene.tabs.${tab}`) }}
                    </button>
                  </nav>
                  <div v-if="shownTab(item) === 'layout'" class="editor-grid">
                    <!-- Locked in OBS: where it sits and how large it is stay as they are; its order still changes. -->
                    <template v-if="item.locked">
                      <span></span>
                      <span class="locked-note">
                        <i class="pi pi-lock" aria-hidden="true"></i>{{ t('scene.lockedNote') }}
                        <Button :label="t('scene.unlock')" size="small" text :disabled="busy.has(item.id)" @click="change(item, { locked: false })" />
                      </span>
                    </template>
                    <template v-if="placements.get(item.id)">
                      <span>{{ t('scene.position') }}</span>
                      <span class="fields">
                        <InputNumber
                          :model-value="placements.get(item.id)!.x"
                          :max-fraction-digits="1"
                          :disabled="busy.has(item.id) || item.locked"
                          :aria-label="t('scene.x', { item: item.source })"
                          class="coord"
                          @update:model-value="(x: number | null) => x !== null && place(item, { x })"
                        />
                        <InputNumber
                          :model-value="placements.get(item.id)!.y"
                          :max-fraction-digits="1"
                          :disabled="busy.has(item.id) || item.locked"
                          :aria-label="t('scene.y', { item: item.source })"
                          class="coord"
                          @update:model-value="(y: number | null) => y !== null && place(item, { y })"
                        />
                      </span>
                      <!-- Size on the canvas; 等比例 keeps its width ÷ height (on unless turned off). -->
                      <span>{{ t('scene.size') }}</span>
                      <span class="fields">
                        <InputNumber
                          :model-value="Math.round(placements.get(item.id)!.width)"
                          :min="1"
                          :use-grouping="false"
                          :disabled="busy.has(item.id) || item.locked || !placements.get(item.id)!.width"
                          :aria-label="t('scene.width', { item: item.source })"
                          class="coord"
                          @update:model-value="(v: number | null) => v !== null && resize(item, 'width', v)"
                        />
                        <Button
                          :label="t('scene.ratio')"
                          :icon="unlocked.has(item.id) ? 'pi pi-lock-open' : 'pi pi-lock'"
                          :aria-pressed="!unlocked.has(item.id)"
                          size="small"
                          outlined
                          :disabled="busy.has(item.id) || item.locked"
                          @click="toggleRatio(item)"
                        />
                        <InputNumber
                          :model-value="Math.round(placements.get(item.id)!.height)"
                          :min="1"
                          :use-grouping="false"
                          :disabled="busy.has(item.id) || item.locked || !placements.get(item.id)!.height"
                          :aria-label="t('scene.height', { item: item.source })"
                          class="coord"
                          @update:model-value="(v: number | null) => v !== null && resize(item, 'height', v)"
                        />
                      </span>
                      <span>{{ t('scene.align') }}</span>
                      <span class="fields">
                        <Select
                          :model-value="placements.get(item.id)!.align"
                          :options="ALIGNS"
                          :option-label="(a: Align) => t(`scene.aligns.${a}`)"
                          :disabled="busy.has(item.id) || item.locked"
                          :aria-label="t('scene.alignOf', { item: item.source })"
                          @update:model-value="(align: Align) => place(item, { align })"
                        />
                        <span v-if="outside(placements.get(item.id)!)" class="warn">{{ t('scene.outside') }}</span>
                      </span>
                      <!-- Against the canvas (inside it) or another item (outside it), once: nothing follows later. -->
                      <template v-if="placements.get(item.id)!.width">
                        <span>{{ t('scene.relative') }}</span>
                        <span class="fields">
                          <Select
                            v-model="rel.ref"
                            :options="refOptions(item)"
                            option-label="label"
                            option-value="value"
                            :aria-label="t('scene.relativeRef', { item: item.source })"
                            :disabled="busy.has(item.id) || item.locked"
                          />
                          <SelectButton
                            v-model="rel.side"
                            :options="SIDES"
                            :option-label="(v: Side) => t(`scene.sides.${v}`)"
                            :allow-empty="false"
                            :aria-label="t('scene.relativeSide')"
                            :disabled="busy.has(item.id) || item.locked"
                          />
                          <SelectButton
                            v-model="rel.cross"
                            :options="CROSSES"
                            :option-label="(v: Cross) => t(`scene.crosses.${v}`)"
                            :allow-empty="false"
                            :aria-label="t('scene.relativeCross')"
                            :disabled="busy.has(item.id) || item.locked || rel.side === 'center'"
                          />
                          <InputNumber
                            v-model="rel.ox"
                            :use-grouping="false"
                            :aria-label="t('scene.offsetX')"
                            class="coord"
                            :disabled="busy.has(item.id) || item.locked"
                          />
                          <InputNumber
                            v-model="rel.oy"
                            :use-grouping="false"
                            :aria-label="t('scene.offsetY')"
                            class="coord"
                            :disabled="busy.has(item.id) || item.locked"
                          />
                          <Button :label="t('scene.apply')" size="small" outlined :disabled="busy.has(item.id) || item.locked" @click="alignOnce(item)" />
                        </span>
                        <span></span>
                        <span class="hint">{{ t('scene.relativeHint') }}</span>
                      </template>
                    </template>
                    <p v-else class="muted">{{ t('scene.noPlacement') }}</p>
                    <!-- Order: a number counted from the front (1), or right below another item. -->
                    <span>{{ t('scene.order') }}</span>
                    <span class="fields">
                      <label :for="`index-${item.id}`">{{ t('scene.indexBefore') }}</label>
                      <InputNumber
                        :input-id="`index-${item.id}`"
                        :model-value="i + 1"
                        :min="1"
                        :max="current.items.length"
                        :use-grouping="false"
                        :disabled="busy.has(item.id)"
                        class="index"
                        @update:model-value="(n: number | null) => n !== null && move(item, n - 1)"
                      />
                      <span>{{ t('scene.indexAfter') }}</span>
                      <Select
                        :model-value="null"
                        :options="current.items.filter((o) => o.id !== item.id)"
                        :option-label="(o: SceneItem) => o.source"
                        :placeholder="t('scene.belowPick')"
                        :input-id="`below-${item.id}`"
                        :aria-label="t('scene.belowLabel', { item: item.source })"
                        :disabled="busy.has(item.id)"
                        @update:model-value="(o: SceneItem) => moveBelow(item, o)"
                      />
                      <span>{{ t('scene.belowAfter') }}</span>
                    </span>
                  </div>
                  <!-- 變形: turn, flip, crop and fit; only the part changed is sent, the rest of OBS's transform stays. -->
                  <div v-else-if="shownTab(item) === 'transform'" class="editor-grid">
                    <template v-if="item.locked">
                      <span></span>
                      <span class="locked-note">
                        <i class="pi pi-lock" aria-hidden="true"></i>{{ t('scene.lockedNote') }}
                        <Button :label="t('scene.unlock')" size="small" text :disabled="busy.has(item.id)" @click="change(item, { locked: false })" />
                      </span>
                    </template>
                    <template v-if="transforms.get(item.id)">
                      <span>{{ t('scene.rotation') }}</span>
                      <span class="fields">
                        <InputNumber
                          :model-value="transforms.get(item.id)!.rotation"
                          :min="-360"
                          :max="360"
                          :use-grouping="false"
                          :aria-label="t('scene.rotationOf', { item: item.source })"
                          :disabled="busy.has(item.id) || item.locked"
                          class="index"
                          @update:model-value="(v: number | null) => v !== null && transform(item, { rotation: v })"
                        />
                        <Button
                          v-for="d in [0, 90, 180, 270]"
                          :key="d"
                          :label="`${d}°`"
                          size="small"
                          text
                          :aria-pressed="transforms.get(item.id)!.rotation === d"
                          :disabled="busy.has(item.id) || item.locked"
                          @click="transform(item, { rotation: d })"
                        />
                      </span>
                      <span>{{ t('scene.flip') }}</span>
                      <span class="fields">
                        <Button
                          v-for="f in (['flip_h', 'flip_v'] as const)"
                          :key="f"
                          :label="t(`scene.${f}`)"
                          size="small"
                          outlined
                          :aria-pressed="transforms.get(item.id)![f]"
                          :disabled="busy.has(item.id) || item.locked"
                          @click="transform(item, { [f]: !transforms.get(item.id)![f] })"
                        />
                      </span>
                      <span>{{ t('scene.crop') }}</span>
                      <span class="fields">
                        <template v-for="side in (['top', 'right', 'bottom', 'left'] as const)" :key="side">
                          <span>{{ t(`scene.cropSides.${side}`) }}</span>
                          <InputNumber
                            :model-value="transforms.get(item.id)!.crop[side]"
                            :min="0"
                            :use-grouping="false"
                            :aria-label="t('scene.cropOf', { item: item.source, side: t(`scene.cropSides.${side}`) })"
                            :disabled="busy.has(item.id) || item.locked"
                            class="index"
                            @update:model-value="(v: number | null) => v !== null && transform(item, { crop: { ...transforms.get(item.id)!.crop, [side]: Math.max(0, v) } })"
                          />
                        </template>
                      </span>
                      <span>{{ t('scene.bounds') }}</span>
                      <span class="fields">
                        <Select
                          :model-value="transforms.get(item.id)!.bounds"
                          :options="BOUNDS"
                          :option-label="(b: BoundsKind) => t(`scene.boundsKinds.${b}`)"
                          :aria-label="t('scene.boundsOf', { item: item.source })"
                          :disabled="busy.has(item.id) || item.locked"
                          @update:model-value="(b: BoundsKind) => transform(item, { bounds: b })"
                        />
                        <span class="hint">{{ t(transforms.get(item.id)!.bounds === 'none' ? 'scene.boundsNone' : 'scene.boundsBox') }}</span>
                      </span>
                      <span></span>
                      <span class="fields">
                        <Button
                          :label="t('scene.resetTransform')"
                          icon="pi pi-undo"
                          size="small"
                          text
                          :disabled="busy.has(item.id) || item.locked"
                          @click="transform(item, RESET)"
                        />
                      </span>
                    </template>
                    <p v-else class="muted">{{ t('app.loading') }}</p>
                  </div>
                  <!-- 來源設定: the source's own settings, shared by every scene that shows it. -->
                  <div v-else-if="shownTab(item) === 'source'" class="editor-grid">
                    <span></span>
                    <span class="hint">{{ t('scene.sourceShared', { source: item.source }) }}</span>
                    <template v-if="sources.get(item.source)">
                      <template v-if="sources.get(item.source)!.family === 'other'">
                        <span></span>
                        <span class="muted">{{ t('scene.sourceInObs') }}</span>
                      </template>
                      <template v-if="sources.get(item.source)!.url_hidden">
                        <span></span>
                        <span class="muted">{{ t('scene.sourceUrlHidden') }}</span>
                      </template>
                      <!-- Browser: this Host's overlay address, the page's own size, custom CSS, shut down when hidden. -->
                      <template v-if="sources.get(item.source)!.family === 'browser'">
                        <template v-if="!sources.get(item.source)!.url_hidden">
                          <span>{{ t('scene.src.url') }}</span>
                          <InputText
                            :model-value="String(field(item, 'url') ?? '')"
                            :aria-label="t('scene.src.url')"
                            class="wide"
                            @change="(e: Event) => setField(item, 'url', (e.target as HTMLInputElement).value)"
                          />
                        </template>
                        <span>{{ t('scene.src.pageSize') }}</span>
                        <span class="fields">
                          <InputNumber
                            v-for="k in (['width', 'height'] as const)"
                            :key="k"
                            :model-value="Number(field(item, k) ?? 0)"
                            :min="1"
                            :max="16384"
                            :use-grouping="false"
                            :aria-label="t(`scene.src.${k}`)"
                            class="coord"
                            @update:model-value="(v: number | null) => v !== null && setField(item, k, v)"
                          />
                          <span class="hint">{{ t('scene.src.pageSizeHint') }}</span>
                        </span>
                        <span>{{ t('scene.src.css') }}</span>
                        <Textarea
                          :model-value="String(field(item, 'css') ?? '')"
                          :aria-label="t('scene.src.css')"
                          rows="2"
                          spellcheck="false"
                          class="wide code"
                          @change="(e: Event) => setField(item, 'css', (e.target as HTMLTextAreaElement).value)"
                        />
                        <span></span>
                        <span class="fields">
                          <Checkbox
                            :model-value="Boolean(field(item, 'shutdown'))"
                            :input-id="`shutdown-${item.id}`"
                            binary
                            @update:model-value="(v: boolean) => setField(item, 'shutdown', v)"
                          />
                          <label :for="`shutdown-${item.id}`">{{ t('scene.src.shutdown') }}</label>
                          <Button :label="t('scene.src.refresh')" icon="pi pi-refresh" size="small" outlined @click="refreshSource(item)" />
                        </span>
                      </template>
                      <!-- Text: what it says, its font and size, its color. -->
                      <template v-else-if="sources.get(item.source)!.family === 'text'">
                        <span>{{ t('scene.src.text') }}</span>
                        <Textarea
                          :model-value="String(field(item, 'text') ?? '')"
                          :aria-label="t('scene.src.text')"
                          rows="2"
                          class="wide"
                          @change="(e: Event) => setField(item, 'text', (e.target as HTMLTextAreaElement).value)"
                        />
                        <span>{{ t('scene.src.font') }}</span>
                        <span class="fields">
                          <InputText
                            :model-value="String(fontOf(item).face ?? '')"
                            :aria-label="t('scene.src.fontFace')"
                            @change="(e: Event) => setField(item, 'font', { ...fontOf(item), face: (e.target as HTMLInputElement).value })"
                          />
                          <InputNumber
                            :model-value="Number(fontOf(item).size ?? 36)"
                            :min="1"
                            :max="1000"
                            :use-grouping="false"
                            :aria-label="t('scene.src.fontSize')"
                            class="index"
                            @update:model-value="(v: number | null) => v !== null && setField(item, 'font', { ...fontOf(item), size: v })"
                          />
                          <input
                            type="color"
                            class="color"
                            :value="textColor(item)"
                            :aria-label="t('scene.src.color')"
                            @change="(e: Event) => setTextColor(item, (e.target as HTMLInputElement).value)"
                          />
                        </span>
                      </template>
                      <!-- Capture and audio: choices come from OBS; the window shows only for a specific window. -->
                      <template v-else-if="['video_capture', 'app_audio', 'game'].includes(sources.get(item.source)!.family)">
                        <template v-for="key in listedFields(item)" :key="key">
                          <span>{{ t(`scene.src.${key}`) }}</span>
                          <Select
                            :model-value="field(item, key)"
                            :options="options.get(`${item.source}/${key}`) ?? []"
                            option-label="name"
                            option-value="value"
                            :placeholder="t('scene.src.pick')"
                            :aria-label="t(`scene.src.${key}`)"
                            class="wide"
                            @show="loadOptions(item, key)"
                            @update:model-value="(v: unknown) => setField(item, key, v, key === 'resolution' ? { res_type: 1 } : {})"
                          />
                        </template>
                      </template>
                      <!-- Image: its file, typed in when OBS runs on this computer; elsewhere OBS changes it. -->
                      <template v-else-if="sources.get(item.source)!.family === 'image'">
                        <span>{{ t('scene.src.file') }}</span>
                        <span class="fields">
                          <InputText
                            :model-value="String(field(item, 'file') ?? '')"
                            :aria-label="t('scene.src.file')"
                            :readonly="!obsSources?.local"
                            class="wide"
                            @change="(e: Event) => setField(item, 'file', (e.target as HTMLInputElement).value)"
                          />
                          <span v-if="!obsSources?.local" class="hint">{{ t('scene.src.fileRemote') }}</span>
                        </span>
                      </template>
                    </template>
                    <p v-else class="muted">{{ t('app.loading') }}</p>
                  </div>
                </td>
              </tr>
            </template>
          </tbody>
        </table>
        <Menu id="move-menu" ref="moveMenu" :model="moveItems" popup />
        </div>
      </section>
    </div>
  </FullPage>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputNumber from 'primevue/inputnumber'
import Menu from 'primevue/menu'
import type { MenuItem } from 'primevue/menuitem'
import Select from 'primevue/select'
import SelectButton from 'primevue/selectbutton'
import Checkbox from 'primevue/checkbox'
import InputText from 'primevue/inputtext'
import Textarea from 'primevue/textarea'
import ToggleSwitch from 'primevue/toggleswitch'
import { useToast } from 'primevue/usetoast'
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { errorMessage } from '../i18n'
import { ApiError, type Schemas, api, client } from './api'
import FullPage from './FullPage.vue'
import { errorToast, servicesDown, successToast, useLiveEvents } from './home'
import { addOverlay, overlayName, overlayRows, useObsSources } from './obsSources'
import { type Cross, type Side, alignedCorner, boxOf, pointOf } from './relativeAlign'
import SceneList from './SceneList.vue'
import { colorOf, hexOf } from './sourceColor'
import { type SceneItem, kindKey, obsReady, setStudioMode, transitionNow, updateItem } from './scenes'

const { t } = useI18n()
const toast = useToast()
const live = useLiveEvents()
const state = computed(() => live.value.obsState)
// The scene this page edits: the preview in studio mode, else the one on air.
const editingPreview = computed(() => !!state.value?.studioMode && !!state.value.preview)
const target = computed(() => (editingPreview.value ? state.value?.preview : state.value?.scene) ?? undefined)
const current = computed(() => state.value?.scenes.find((s) => s.name === target.value))
// Items with a command in flight; their switches wait for it.
const busy = ref(new Set<number>())
// A switch shows the change it asked for until OBS reports it; a refused one is rebuilt from OBS's state.
const resets = ref(0)

const kindText = (kind: string) => {
  const key = kindKey(kind)
  return key ? t(key) : kind || '—'
}
const report = (e: unknown) => toast.add(errorToast(e instanceof ApiError ? errorMessage(e.code) : String(e)))

// Sources showing an overlay that has another source too; read again as OBS's scenes and items change.
const obsSources = useObsSources()
watch(state, () => void useObsSources(true))
const duplicates = computed(
  () =>
    new Set(
      overlayRows(obsSources.value?.sources ?? [])
        .filter((r) => r.sources.length > 1)
        .flatMap((r) => r.sources.map((s) => s.name)),
    ),
)

// Studio mode: the transition picked here, starting from the one in use in OBS.
const transition = ref<string | null>(null)
watch(
  () => state.value?.transition,
  (now) => (transition.value = now ?? null),
  { immediate: true },
)
const studioBusy = ref(false)
async function studio(command: () => Promise<unknown>) {
  studioBusy.value = true
  try {
    await command()
  } catch (e) {
    report(e)
  } finally {
    studioBusy.value = false
  }
}

// 加入 Overlay: the built-in overlays not in this scene yet, with their suggested size.
const addMenu = ref<InstanceType<typeof Menu>>()
const adding = ref(false)
const addable = computed<MenuItem[]>(() => {
  const scene = target.value
  if (!scene) return []
  return overlayRows(obsSources.value?.sources ?? [])
    .filter((r) => r.url && !r.scenes.includes(scene))
    .map((r) => ({
      label: `${overlayName(r, t)}　${r.entry.size === 'audio' ? t('obsSources.audioOnly') : r.entry.size}`,
      command: async () => {
        adding.value = true
        try {
          await addOverlay(scene, r, t)
        } catch (e) {
          report(e)
        } finally {
          adding.value = false
          void useObsSources(true)
        }
      },
    }))
})

// The item whose editor is open, and the tab shown (kept while another item opens, if that one has it too).
type EditorTab = 'layout' | 'transform' | 'source'
const open = ref<number | null>(null)
const editorTab = ref<EditorTab>('layout')
// What each item can edit; later tabs join as they are built.
// 變形 needs something to turn: not a group, and not an item with sound only.
const tabsOf = (item: SceneItem): EditorTab[] => {
  if (item.kind === 'group') return ['layout']
  return placements.value.get(item.id)?.width ? ['layout', 'transform', 'source'] : ['layout', 'source']
}
const shownTab = (item: SceneItem) => (tabsOf(item).includes(editorTab.value) ? editorTab.value : 'layout')
// Order: OBS counts from the bottom, the admin API front to back from 0 (P2-12); `move` takes that index. The moved
// row flashes and keeps the keyboard once OBS reports the new order.
const moved = ref<number | null>(null)
let movedTimer: ReturnType<typeof setTimeout> | undefined
async function move(item: SceneItem, to: number) {
  const items = current.value?.items ?? []
  const target = Math.max(0, Math.min(items.length - 1, to))
  if (items.findIndex((o) => o.id === item.id) === target) return
  if (!(await change(item, { position: target }))) return
  moved.value = item.id
  clearTimeout(movedTimer)
  movedTimer = setTimeout(() => (moved.value = null), 1600)
}
watch(current, () => {
  if (moved.value === null) return
  const id = moved.value
  void nextTick(() => document.querySelector<HTMLElement>(`tr[data-item="${id}"] .item-name`)?.focus())
})
const moveBelow = (item: SceneItem, other: SceneItem) => {
  const items = current.value?.items ?? []
  const from = items.findIndex((o) => o.id === item.id)
  const at = items.findIndex((o) => o.id === other.id)
  return move(item, from < at ? at : at + 1)
}

// The row menu: to the front or the back, or right below another item (opens the editor at that field).
const moveMenu = ref<InstanceType<typeof Menu>>()
const moveItems = ref<MenuItem[]>([])
function openMoveMenu(e: Event, item: SceneItem, i: number) {
  const last = (current.value?.items.length ?? 1) - 1
  moveItems.value = [
    { label: t('scene.toFront'), icon: 'pi pi-angle-double-up', disabled: i === 0, command: () => move(item, 0) },
    { label: t('scene.toBack'), icon: 'pi pi-angle-double-down', disabled: i === last, command: () => move(item, last) },
    {
      label: t('scene.belowMenu'),
      icon: 'pi pi-arrow-down-right',
      command: () => {
        open.value = item.id
        editorTab.value = 'layout'
        void nextTick(() => document.getElementById(`below-${item.id}`)?.focus())
      },
    },
  ]
  moveMenu.value?.toggle(e)
}

// Drag a row by anywhere on it; a line shows where it lands.
const dragged = ref<number | null>(null)
const over = ref<number | null>(null)
const dropSide = ref<'before' | 'after'>('before')
function onDragstart(e: DragEvent, item: SceneItem) {
  dragged.value = item.id
  e.dataTransfer?.setData('text/plain', String(item.id))
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}
function onDragover(e: DragEvent, item: SceneItem) {
  if (dragged.value === null || dragged.value === item.id) return
  e.preventDefault()
  const box = (e.currentTarget as HTMLElement).getBoundingClientRect()
  dropSide.value = e.clientY < box.top + box.height / 2 ? 'before' : 'after'
  over.value = item.id
}
function onDrop(e: DragEvent, target: SceneItem) {
  const from = current.value?.items.find((o) => o.id === dragged.value)
  dragged.value = over.value = null
  if (!from || from.id === target.id) return
  e.preventDefault()
  const items = current.value!.items
  const fromAt = items.indexOf(from)
  let at = items.indexOf(target) + (dropSide.value === 'after' ? 1 : 0)
  if (fromAt < at) at--
  void move(from, at)
}

const geoText = (item: SceneItem) => {
  const p = placements.value.get(item.id)
  if (!p) return '—'
  const at = `${Math.round(p.x)}, ${Math.round(p.y)}`
  const tr = transforms.value.get(item.id)
  const turned = tr?.rotation ? ` · ${tr.rotation}°` : ''
  const flipped = tr && (tr.flip_h || tr.flip_v) ? ` · ${t('scene.flipped')}` : ''
  return (p.width ? `${at} · ${Math.round(p.width)} × ${Math.round(p.height)}` : at) + turned + flipped
}

// 變形: read when its tab opens, changed part by part (the rest of OBS's transform stays).
type BoundsKind = Schemas['Bounds']
const BOUNDS: BoundsKind[] = ['none', 'scale_inner', 'scale_outer', 'stretch', 'scale_to_width', 'scale_to_height', 'max_only']
const RESET: Schemas['TransformChange'] = { rotation: 0, flip_h: false, flip_v: false, crop: { top: 0, right: 0, bottom: 0, left: 0 }, bounds: 'none' }
const transforms = ref(new Map<number, Schemas['ItemTransform']>())
watch([open, editorTab, target], async ([id]) => {
  const item = current.value?.items.find((o) => o.id === id)
  if (!item || shownTab(item) !== 'transform' || !target.value) return
  try {
    const got = await api(client.GET('/api/admin/obs/transform', { params: { query: { scene: target.value, item: item.id } } }))
    transforms.value = new Map(transforms.value).set(item.id, got)
  } catch (e) {
    report(e)
  }
})
// The source's form: a field changed is sent alone (the Host merges it) and kept here once OBS has it.
const field = (item: SceneItem, key: string) => sources.value.get(item.source)?.fields[key] as unknown
const fontOf = (item: SceneItem) => (field(item, 'font') ?? {}) as { face?: string; size?: number }
// Shown at once, so the next change (the font size after its face) starts from it; a source's writes go one after
// another, so OBS ends with the last; a refused one goes back unless changed again meanwhile.
const writes = new Map<string, Promise<unknown>>()
// `also`: fields that must change along with it (a capture resolution applies only in its custom mode).
async function setField(item: SceneItem, key: string, value: unknown, also: Record<string, unknown> = {}) {
  const now = sources.value.get(item.source)
  if (!now) return
  const before = now.fields[key]
  const show = (v: unknown) => {
    const latest = sources.value.get(item.source)
    if (latest) sources.value = new Map(sources.value).set(item.source, { ...latest, fields: { ...latest.fields, ...also, [key]: v } as never })
  }
  show(value)
  // `fields` is a free-form object in the API description; its keys are checked by the Host.
  const write = (writes.get(item.source) ?? Promise.resolve()).then(() =>
    api(client.PUT('/api/admin/obs/source', { body: { input: item.source, fields: { ...also, [key]: value } as never } })),
  )
  writes.set(item.source, write.catch(() => {}))
  try {
    await write
  } catch (e) {
    if (sources.value.get(item.source)?.fields[key] === value) show(before)
    report(e)
  }
}
// The Windows text source keeps `color`, the FreeType one `color1`.
const colorKey = (item: SceneItem) => (field(item, 'color1') !== undefined && field(item, 'color') === undefined ? 'color1' : 'color')
const textColor = (item: SceneItem) => hexOf(Number(field(item, colorKey(item)) ?? 0xffffff))
const setTextColor = (item: SceneItem, hex: string) => setField(item, colorKey(item), colorOf(hex, colorKey(item) === 'color1'))
// Capture and audio choices, asked when a list opens (devices and windows change while OBS runs).
const LISTED: Record<string, string[]> = {
  video_capture: ['video_device_id', 'resolution'],
  app_audio: ['window'],
  game: ['capture_mode', 'window'],
}
const options = ref(new Map<string, Schemas['SourceOption'][]>())
const listedFields = (item: SceneItem) => {
  const s = sources.value.get(item.source)
  const keys = LISTED[s?.family ?? ''] ?? []
  // Game capture's window applies only to capturing a specific window.
  return s?.family === 'game' && field(item, 'capture_mode') !== 'window' ? keys.filter((k) => k !== 'window') : keys
}
async function loadOptions(item: SceneItem, key: string) {
  try {
    const got = await api(client.GET('/api/admin/obs/source/options', { params: { query: { input: item.source, property: key } } }))
    options.value = new Map(options.value).set(`${item.source}/${key}`, got)
  } catch (e) {
    report(e)
  }
}

async function refreshSource(item: SceneItem) {
  try {
    await api(client.POST('/api/admin/obs/source/refresh', { body: { input: item.source } }))
    toast.add(successToast(t('scene.src.refreshed', { source: item.source })))
  } catch (e) {
    report(e)
  }
}

// 來源設定: read when its tab opens; only the fields of the source's form come back.
const sources = ref(new Map<string, Schemas['SourceSettings']>())
watch([open, editorTab], async ([id]) => {
  const item = current.value?.items.find((o) => o.id === id)
  if (!item || shownTab(item) !== 'source') return
  try {
    const got = await api(client.GET('/api/admin/obs/source', { params: { query: { input: item.source } } }))
    sources.value = new Map(sources.value).set(item.source, got)
  } catch (e) {
    report(e)
  }
})

async function transform(item: SceneItem, what: Schemas['TransformChange']) {
  const now = transforms.value.get(item.id)
  if (!now) return
  if (!(await change(item, { transform: what }))) return
  const next = { ...now, ...Object.fromEntries(Object.entries(what).filter(([, v]) => v !== undefined && v !== null)) }
  transforms.value = new Map(transforms.value).set(item.id, next as Schemas['ItemTransform'])
  // Crop and the bounding box change its size on the canvas, which OBS sends no event for.
  if (what.crop || what.bounds) geometry.value++
}

// Relative alignment (computed once): the reference, the side, along it, and an offset.
const SIDES: Side[] = ['top', 'bottom', 'left', 'right', 'center']
const CROSSES: Cross[] = ['start', 'middle', 'end']
const rel = ref<{ ref: string; side: Side; cross: Cross; ox: number; oy: number }>({ ref: 'canvas', side: 'top', cross: 'middle', ox: 0, oy: 0 })
const refOptions = (item: SceneItem) => [
  { value: 'canvas', label: t('scene.canvas') },
  ...(current.value?.items ?? [])
    .filter((o) => o.id !== item.id && placements.value.get(o.id)?.width)
    .map((o) => ({ value: String(o.id), label: o.source })),
]
function alignOnce(item: SceneItem) {
  const own = placements.value.get(item.id)
  if (!own?.width || !canvas.value) return
  const r = rel.value
  const other = r.ref === 'canvas' ? undefined : placements.value.get(Number(r.ref))
  const refBox = other ? boxOf(other, other) : { left: 0, top: 0, width: canvas.value.width, height: canvas.value.height }
  const corner = alignedCorner(own, refBox, r.side, r.cross, { x: r.ox ?? 0, y: r.oy ?? 0 }, !other)
  void place(item, pointOf(corner.left, corner.top, own.align, own))
}

// Size: with 等比例 (the default) the other side follows, at least 1; OBS gets the new scale.
const unlocked = ref(new Set<number>())
function toggleRatio(item: SceneItem) {
  const next = new Set(unlocked.value)
  if (!next.delete(item.id)) next.add(item.id)
  unlocked.value = next
}
async function resize(item: SceneItem, side: 'width' | 'height', value: number) {
  const now = placements.value.get(item.id)
  if (!now?.width || !now.height) return
  const v = Math.max(1, value)
  const ratio = now.width / now.height
  const size = unlocked.value.has(item.id)
    ? { width: side === 'width' ? v : now.width, height: side === 'height' ? v : now.height }
    : side === 'width'
      ? { width: v, height: Math.max(1, Math.round(v / ratio)) }
      : { width: Math.max(1, Math.round(v * ratio)), height: v }
  if (await change(item, { size })) placements.value = new Map(placements.value).set(item.id, { ...now, ...size })
}

type Align = Schemas['Align']
const ALIGNS: Align[] = ['top_left', 'top', 'top_right', 'left', 'center', 'right', 'bottom_left', 'bottom', 'bottom_right']
// The canvas and each item's position in the edited scene, read again as OBS's scenes and items change.
const canvas = ref<Schemas['Canvas']>()
const placements = ref(new Map<number, Schemas['Placement'] & Schemas['ItemSize']>())
// Each read's number; only the latest one's answer shows, so an older scene's never replaces the edited one.
let placementRead = 0
// Read again after a change that moves or sizes items without an OBS event.
const geometry = ref(0)
watch(
  [state, target, geometry],
  async ([, scene]) => {
    if (!scene) return
    const mine = ++placementRead
    try {
      const got = await api(client.GET('/api/admin/obs/placements', { params: { query: { scene } } }))
      if (mine !== placementRead) return
      canvas.value = got.canvas
      placements.value = new Map(got.items.map(({ item, ...p }) => [item, p]))
    } catch {
      if (mine === placementRead) placements.value = new Map()
    }
  },
  { immediate: true },
)
const outside = (p: Schemas['Placement']) =>
  !!canvas.value && (p.x < 0 || p.y < 0 || p.x > canvas.value.width || p.y > canvas.value.height)

async function place(item: SceneItem, what: Partial<Schemas['Placement']>) {
  const now = placements.value.get(item.id)
  if (!now) return
  const next = { x: now.x, y: now.y, align: now.align, ...what }
  // OBS sends no event when an item moves, so the next change starts from this one, not the last read.
  if (await change(item, { placement: next })) placements.value = new Map(placements.value).set(item.id, { ...now, ...next })
}

async function change(
  item: SceneItem,
  what: {
    enabled?: boolean
    locked?: boolean
    position?: number
    placement?: Schemas['Placement']
    size?: Schemas['ItemSize']
    transform?: Schemas['TransformChange']
  },
) {
  const scene = target.value
  if (!scene) return false
  busy.value = new Set(busy.value).add(item.id)
  try {
    await updateItem(scene, item.id, what)
    return true
  } catch (e) {
    resets.value++
    report(e)
    return false
  } finally {
    const next = new Set(busy.value)
    next.delete(item.id)
    busy.value = next
  }
}
</script>

<style scoped>
/* Wide and tall enough: the page fits the window and each pane scrolls; the table head stays. */
@media (min-height: 560px) {
  @container view (min-width: 64.0625rem) {
    .scene-panes {
      display: grid;
      grid-template-columns: minmax(16rem, 22rem) minmax(0, 1fr);
      gap: 1rem;
      height: calc(100vh - var(--appbar-h, 0px) - var(--page-top-h, 0px) - 3rem);
      min-height: 24rem;
    }

    .pane {
      display: flex;
      flex-direction: column;
      min-height: 0;
      overflow-y: auto;
    }

    .items-scroll {
      min-height: 0;
      overflow: auto;
    }

    .items-scroll thead th {
      position: sticky;
      top: 0;
      z-index: 1;
      background: var(--p-surface-950);
    }
  }
}

/* One column: the scene list keeps to about three rows, so the items are near. */
@container view (max-width: 64rem) {
  .pane:first-child :deep(.scenes) {
    max-height: 11.5rem;
    overflow-y: auto;
  }
}

.items-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.25rem 0.75rem;
}

.items-head h2 {
  margin: 0;
}

.stag {
  padding: 0 0.5rem;
  border-radius: 999px;
  font-size: 0.75rem;
}

.stag.live {
  background: color-mix(in srgb, var(--p-red-400) 20%, transparent);
  color: var(--p-red-300);
}

.stag.preview {
  background: color-mix(in srgb, var(--p-green-400) 20%, transparent);
  color: var(--p-green-300);
}

.table {
  width: 100%;
  max-width: 60rem;
}

.item-name {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.125rem 0.25rem;
  border: 0;
  border-radius: 4px;
  background: none;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}

.item-name:hover {
  background: var(--p-surface-800);
}

.item-name .pi {
  font-size: 0.75rem;
  color: var(--p-text-muted-color);
  transition: transform 0.15s;
}

.item-name[aria-expanded='true'] .pi {
  transform: rotate(90deg);
}

tr.open > *,
tr.editor > td {
  background: var(--p-surface-900);
}

tbody tr[draggable='true'] {
  cursor: grab;
}

tr.dragging {
  opacity: 0.4;
}

tr.drop-before > * {
  box-shadow: inset 0 2px 0 var(--p-primary-color);
}

tr.drop-after > * {
  box-shadow: inset 0 -2px 0 var(--p-primary-color);
}

tr.moved > * {
  animation: moved 1.6s ease-out;
}

@keyframes moved {
  from {
    background: color-mix(in srgb, var(--p-primary-color) 25%, transparent);
  }
}

@media (prefers-reduced-motion: reduce) {
  tr.moved > * {
    animation: none;
    outline: 1px solid var(--p-primary-color);
  }
}

.wide {
  width: min(100%, 28rem);
}

.code {
  font-family: ui-monospace, Consolas, monospace;
  font-size: 0.8125rem;
}

.color {
  width: 2.5rem;
  height: 2rem;
  padding: 0.125rem;
  border: 1px solid var(--p-surface-600);
  border-radius: 6px;
  background: none;
}

.hint {
  margin-top: -0.25rem;
  color: var(--p-text-muted-color);
  font-size: 0.8125rem;
}

.locked-note {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  color: var(--p-yellow-300);
  font-size: 0.875rem;
}

.index :deep(.p-inputnumber-input) {
  width: 4rem;
}

.geo {
  font-variant-numeric: tabular-nums;
  color: var(--p-text-muted-color);
}

.editor-tabs {
  display: flex;
  gap: 0.25rem;
  margin-bottom: 0.5rem;
  border-bottom: 1px solid var(--p-surface-800);
}

.editor-tabs button {
  padding: 0.375rem 0.75rem;
  border: 0;
  border-bottom: 2px solid transparent;
  background: none;
  color: var(--p-text-muted-color);
  font: inherit;
  cursor: pointer;
}

.editor-tabs button[aria-current='page'] {
  border-bottom-color: var(--p-primary-color);
  color: var(--p-primary-color);
}

.editor-grid {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 0.5rem 1rem;
  align-items: center;
}

.editor-grid > span:nth-child(odd) {
  color: var(--p-text-muted-color);
  font-size: 0.875rem;
}

.fields {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}

caption {
  text-align: start;
  color: var(--p-text-muted-color);
  padding-bottom: 0.25rem;
}

tbody th {
  font-weight: normal;
}

.dup {
  margin-inline-start: 0.5rem;
  padding: 0 0.375rem;
  border: 1px solid var(--p-yellow-700);
  border-radius: 999px;
  color: var(--p-yellow-200);
  font-size: 0.75rem;
  font-weight: 400;
  text-decoration: none;
}

.move {
  white-space: nowrap;
}

.coord :deep(.p-inputnumber-input) {
  width: 5.5rem;
}

.warn {
  display: block;
  color: var(--p-yellow-300);
  font-size: 0.8125rem;
}

.add {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1rem;
  align-items: center;
  margin-left: auto;
}

.muted {
  color: var(--p-text-muted-color);
}

.studio {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 1rem;
  align-items: center;
}

.switch {
  display: inline-flex;
  gap: 0.5rem;
  align-items: center;
}
</style>
