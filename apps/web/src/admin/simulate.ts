import { type Schemas, api, client } from './api'

/*
 * Test events for the overlays, shared by the home drawers and the 模擬事件 dialog. The Host marks them
 * `source=simulation`, so they only show in the overlays and are never written to any record.
 */

export const simulateChat = (body: Schemas['SimulatedChat']) => api(client.POST('/api/admin/simulate/chat', { body }))

export const simulateAlert = (body: Schemas['SimulatedAlert']) => api(client.POST('/api/admin/simulate/alert', { body }))

export const simulateCheckin = (body: Schemas['SimulatedCheckin']) =>
  api(client.POST('/api/admin/simulate/checkin', { body }))

/** Shows a fixed test value on the counter or marathon timer overlay for a few seconds; no counter changes. */
export const testOverlay = (overlay: Schemas['TestOverlay']) =>
  api(client.POST('/api/admin/simulate/overlay-test', { body: { overlay } }))

/** Plays the first playable track of the playlist chosen last on the music overlay for 5 seconds; the player stays as it is. */
export const testMusic = () => api(client.POST('/api/admin/music/player/test'))

/** Plays a library sound on the sound overlay. */
export const testSound = (id: string) => api(client.POST('/api/admin/sounds/{id}/play', { params: { path: { id } } }))

/** The amount each alert kind is simulated with in the dialog (months, gifts, bits, raiders). */
export const ALERT_AMOUNTS: Partial<Record<Schemas['AlertKindName'], number>> = {
  resubscribe: 3,
  gift: 5,
  cheer: 100,
  raid: 20,
}
