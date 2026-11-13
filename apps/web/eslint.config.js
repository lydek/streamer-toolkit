import { globalIgnores } from 'eslint/config'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import pluginVue from 'eslint-plugin-vue'

// Correctness rules only; there is no formatter rule set, so lint never fights over layout.
export default defineConfigWithVueTs(
  { name: 'app/files-to-lint', files: ['**/*.{ts,mts,vue}'] },
  globalIgnores(['**/dist/**', '**/dist-port/**']),
  pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,
)
