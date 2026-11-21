import { i18n } from '../i18n'
import { type Schemas, api, client } from './api'

/** Adds or takes one; the new value arrives as `counter.values`. */
export const stepCounter = (id: string, delta: 1 | -1) =>
  api(client.POST('/api/admin/counters/{id}/add', { params: { path: { id } }, body: { delta } }))

export const resetCounter = (id: string) => api(client.POST('/api/admin/counters/{id}/reset', { params: { path: { id } } }))

/** The home tile's line: the first counter, or that there is none. */
export function counterLine(counters: Schemas['CounterValue'][]): string {
  const { t } = i18n.global
  const first = counters[0]
  return first ? t('counter.line', { name: first.name, value: first.value }) : t('counter.empty')
}
