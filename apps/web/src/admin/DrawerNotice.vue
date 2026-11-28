<template>
  <!-- The drawer's result line: read out as an alert when it is an error, as a status otherwise. -->
  <p v-if="notice" :role="notice.error ? 'alert' : 'status'">{{ notice.text }}</p>
</template>

<script setup lang="ts">
import { useHomeDrawer } from './homeDrawer'

const { notice } = useHomeDrawer()
</script>
