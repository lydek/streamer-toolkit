import { type Schemas, api, client } from './api'

/*
 * OBS scenes and items (P2-12). The page shows `obs.state`: the scene on air changes when OBS reports the switch,
 * and an item's switch that OBS refused goes back to what OBS has.
 */

export type StreamingState = Schemas['StreamingState']
export type SceneItem = Schemas['SceneItem']

/** Whether OBS is connected and its state read (the scene is unknown otherwise). */
export const obsReady = (state: StreamingState | undefined): state is StreamingState & { scene: string } =>
  !!state?.scene

export function switchScene(scene: string) {
  return api(client.PUT('/api/admin/obs/scene', { body: { scene } }))
}

/** `position`: the item's new place, counted front to back as the scene lists its items (0 is the front). */
export function updateItem(
  scene: string,
  item: number,
  change: { enabled?: boolean; locked?: boolean; position?: number },
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

/** The i18n key for an item's kind; `null` for a kind not listed here (shown as OBS names it). */
export function kindKey(kind: string): string | null {
  const key = KINDS[kind.replace(/_v\d+$/, '')]
  return key ? `scene.kinds.${key}` : null
}
