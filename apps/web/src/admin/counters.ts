import { i18n } from '../i18n'
import { type Schemas, api, client } from './api'
import type { Line } from './home'

/** Adds or takes one; the new value arrives as `counter.values`. */
export const stepCounter = (id: string, delta: 1 | -1) =>
  api(client.POST('/api/admin/counters/{id}/add', { params: { path: { id } }, body: { delta } }))

export const resetCounter = (id: string) => api(client.POST('/api/admin/counters/{id}/reset', { params: { path: { id } } }))

/** The home tile: the first counter's value under its name, the others on the line under it. */
export function counterTile(counters: Schemas['CounterValue'][]): Line {
  const { t } = i18n.global
  const [first, ...rest] = counters
  if (!first) return { state: 'ready', text: t('counter.empty') }
  const text = rest.map((c) => t('counter.line', { name: c.name, value: c.value })).join('、')
  return { state: 'ready', num: first.value.toLocaleString('zh-TW'), unit: first.name, text }
}
