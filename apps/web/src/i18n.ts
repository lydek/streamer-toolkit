import { createI18n } from 'vue-i18n'
import zhTW from './locales/zh-TW.json'

// Messages are precompiled by @intlify/unplugin-vue-i18n, so the strict CSP needs no 'unsafe-eval'.
export const i18n = createI18n({
  legacy: false,
  locale: 'zh-TW',
  fallbackLocale: 'zh-TW',
  messages: { 'zh-TW': zhTW },
})

/**
 * Stable error codes from the Host and IPC commands map to `errors.<code>`; `params` from the error body fill it in,
 * and a `module` param also gives that feature's name as `feature`.
 */
export function errorMessage(code: string, params: Record<string, unknown> = {}): string {
  const { t, te } = i18n.global
  const key = `errors.${code}`
  if (!te(key)) return t('errors.unknown', { code })
  const module = `modules.names.${params.module}`
  return t(key, typeof params.module === 'string' && te(module) ? { ...params, feature: t(module) } : params)
}
