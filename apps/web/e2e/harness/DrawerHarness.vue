<template>
  <div class="layout">
    <main>
      <h1>Drawer harness</h1>
      <!-- Stands in for a status light or pending item: an opener outside the tiles that can disappear. -->
      <Button v-if="controls.pending" label="Pending item" text @click="toggle('a', $event.currentTarget)" />
      <div class="tiles">
        <button type="button" data-drawer-tile="a" @click="toggle('a', $event.currentTarget)">Tile A</button>
        <button type="button" data-drawer-tile="b" @click="toggle('b', $event.currentTarget)">Tile B</button>
        <button type="button" data-drawer-arrange>Arrange tiles</button>
      </div>
    </main>

    <AppDrawer id="a" title="Drawer A" full-page="/full">
      <template #actions><Button label="Action" /></template>
      <template #now><p>Now</p></template>
      <template #settings>
        <label for="name">Name</label>
        <InputText id="name" v-model="name" autocomplete="off" @keydown.enter="commitName" @blur="commitName" />
        <label for="color">Color</label>
        <Select v-model="color" input-id="color" :options="['red', 'green', 'blue']" append-to="self" @change="commitColor" />
        <!-- Taller than the window, so the drawer content scrolls. -->
        <div class="filler"></div>
      </template>
    </AppDrawer>
    <AppDrawer id="b" title="Drawer B">
      <template #now><p>B</p></template>
    </AppDrawer>
  </div>
</template>

<script setup lang="ts">
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { ref } from 'vue'
import { ApiError } from '../../src/admin/api'
import AppDrawer from '../../src/admin/AppDrawer.vue'
import { useDrawer, useDrawerSetting } from '../../src/admin/drawer'
import { controls } from './controls'

const drawer = useDrawer()
const toggle = (id: string, el: EventTarget | null) =>
  drawer.current.value === id ? drawer.close() : drawer.open(id, el)

async function save(value: string) {
  if (controls.fail) throw new ApiError('e2e.save_failed')
  controls.saved.push(value)
}
const { value: name, commit: commitName } = useDrawerSetting(ref('old'), save)
const { value: color, commit: commitColor } = useDrawerSetting(ref('red'), save)
</script>

<style scoped>
.layout {
  display: flex;
  align-items: flex-start;
}

.layout > main {
  flex: 1;
  min-width: 0;
}

.tiles {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.filler {
  height: 150vh;
}
</style>
