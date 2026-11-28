import { type Schemas, api, client } from './api'

/*
 * OBS scenes and items (P2-12). The page shows `obs.state`: the scene on air changes when OBS reports the switch,
 * and an item's switch that OBS refused goes back to what OBS has.
 */

export type StreamingState = Schemas['StreamingState']
export type SceneItem = Schemas['SceneItem']

/**
 * A row of the item list: a scene's item, or an item inside one of its groups (shown under the group unless it is
 * folded). `id` is unique on the page (a group's items get negative ones, as their OBS IDs repeat the scene's);
 * commands take `scene` (the group's name for its items) and `obsId`. `num` counts from the front: `9`, `9.1`.
 */
export interface ItemRow extends SceneItem {
  obsId: number
  scene: string
  /** The group it is in. */
  group?: string
  num: string
  /** Its group is hidden, so it does not show either. */
  groupHidden: boolean
}

/** A group item's page-unique row ID. */
export const childId = (group: number, item: number) => -(group * 1_000_000 + item)

export function itemRows(scene: string, items: SceneItem[], folded: Set<string>): ItemRow[] {
  return items.flatMap((item, i) => {
    const row: ItemRow = { ...item, obsId: item.id, scene, num: String(i + 1), groupHidden: false }
    if (item.kind !== 'group' || folded.has(item.source)) return [row]
    const children = (item.children ?? []).map((c, j) => ({
      ...c,
      id: childId(item.id, c.id),
      obsId: c.id,
      scene: item.source,
      group: item.source,
      num: `${i + 1}.${j + 1}`,
      groupHidden: !item.enabled,
    }))
    return [row, ...children]
  })
}

/** Whether OBS is connected and its state read (the scene is unknown otherwise). */
export const obsReady = (state: StreamingState | undefined): state is StreamingState & { scene: string } =>
  !!state?.scene

export function switchScene(scene: string) {
  return api(client.PUT('/api/admin/obs/scene', { body: { scene } }))
}

/** Studio mode: puts `scene` in the preview. */
export function previewScene(scene: string) {
  return api(client.PUT('/api/admin/obs/preview', { body: { scene } }))
}

export function setStudioMode(enabled: boolean) {
  return api(client.PUT('/api/admin/obs/studio', { body: { enabled } }))
}

/** Studio mode: puts the preview on air with `transition` (or the one in use). */
export function transitionNow(transition?: string) {
  return api(client.POST('/api/admin/obs/transition', { body: { transition } }))
}

/** `position`: the item's new place, counted front to back as the scene lists its items (0 is the front). */
export function updateItem(
  scene: string,
  item: number,
  change: {
    enabled?: boolean
    locked?: boolean
    position?: number
    placement?: Schemas['Placement']
    size?: Schemas['ItemSize']
    transform?: Schemas['TransformChange']
  },
) {
  return api(client.PUT('/api/admin/obs/item', { body: { scene, item, ...change } }))
}

/** i18n keys under `scene.kinds` for OBS's common input kinds; versioned kinds (`text_gdiplus_v3`) share a key. */
const KINDS: Record<string, string> = {
  scene: 'scene',
  group: 'group',
  browser_source: 'browser',
  text_gdiplus: 'text',
  text_ft2_source: 'text',
  game_capture: 'game',
  window_capture: 'window',
  monitor_capture: 'monitor',
  dshow_input: 'camera',
  image_source: 'image',
  slideshow: 'slideshow',
  ffmpeg_source: 'media',
  vlc_source: 'media',
  color_source: 'color',
  wasapi_input_capture: 'audioIn',
  wasapi_output_capture: 'audioOut',
  wasapi_process_output_capture: 'appAudio',
}

const KIND_ICONS: Record<string, string> = {
  scene: 'pi pi-images',
  group: 'pi pi-folder',
  browser: 'pi pi-globe',
  text: 'pi pi-align-left',
  game: 'pi pi-play',
  window: 'pi pi-window-maximize',
  monitor: 'pi pi-desktop',
  camera: 'pi pi-camera',
  image: 'pi pi-image',
  slideshow: 'pi pi-images',
  media: 'pi pi-video',
  color: 'pi pi-palette',
  audioIn: 'pi pi-microphone',
  audioOut: 'pi pi-volume-up',
  appAudio: 'pi pi-volume-up',
}

/** The icon beside an item of `kind`, shared by the scene page and its drawer; a box for a kind not listed. */
export const kindIcon = (kind: string) => KIND_ICONS[KINDS[kind.replace(/_v\d+$/, '')] ?? ''] ?? 'pi pi-box'

/** The i18n key for an item's kind; `null` for a kind not listed here (shown as OBS names it). */
export function kindKey(kind: string): string | null {
  const key = KINDS[kind.replace(/_v\d+$/, '')]
  return key ? `scene.kinds.${key}` : null
}
