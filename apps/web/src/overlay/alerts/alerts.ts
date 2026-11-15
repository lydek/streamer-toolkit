import type { components } from '../../api/schema'
import { i18n } from '../../i18n'

export type AlertEvent = components['schemas']['AlertEvent']
export type AlertPayload = AlertEvent['payload']
export type AlertKind = AlertPayload['kind']

/** The kinds in the order the simulator lists them; a full record, so a new kind fails the typecheck here. */
export const ALERT_KINDS = Object.keys({
  follow: 0,
  subscribe: 0,
  resubscribe: 0,
  gift: 0,
  cheer: 0,
  raid: 0,
} satisfies Record<AlertKind, 0>) as AlertKind[]

/** One line saying what happened, e.g. "Viewer 送了 20 份訂閱！". The name is viewer text: show it as text only. */
export function alertTitle(p: AlertPayload): string {
  const { t } = i18n.global
  return t(`alerts.text.${p.kind}`, {
    name: p.userName ?? t('alerts.anonymous'),
    months: p.months,
    count: p.count,
    bits: p.bits,
    viewers: p.viewers,
  })
}
