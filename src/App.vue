<script setup lang="ts">
import { onMounted } from 'vue'
import { preloadTauri, currentWindowLabel } from './tauri'
import { loadPetManifest } from './store/pet'
import PetHost from './components/PetHost.vue'
import PetSettings from './components/PetSettings.vue'

// 多窗口路由：main=宠物窗口，settings=设置窗口，浏览器 dev 默认 main。
// 首帧同步读 __TAURI_INTERNALS__.metadata.currentWindow.label，无竞态。
const winLabel = currentWindowLabel()

onMounted(async () => {
  await preloadTauri()
  await loadPetManifest()
})
</script>

<template>
  <PetHost v-if="winLabel === 'main'" />
  <PetSettings v-else-if="winLabel === 'settings'" />
</template>
