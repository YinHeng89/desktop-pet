<script setup lang="ts">
import { onMounted } from 'vue'
import { preloadTauri, currentWindowLabel } from './tauri'
import { loadPetManifest } from './store/pet'
import { initPlatform } from './shared/platform'
import PetHost from './components/PetHost.vue'
import PetSettings from './components/PetSettings.vue'

// 多窗口路由：main=宠物窗口，settings=设置窗口，浏览器 dev 默认 main。
// 首帧同步读 __TAURI_INTERNALS__.metadata.currentWindow.label，无竞态。
const winLabel = currentWindowLabel()

onMounted(async () => {
  await preloadTauri()
  // 平台探测必须先于宠物清单：PetHost 靠它决定走原生（macOS）还是 DOM 交互方案。
  // 两者无数据依赖，故并行发起。
  await Promise.all([initPlatform(), loadPetManifest()])
})
</script>

<template>
  <PetHost v-if="winLabel === 'main'" />
  <PetSettings v-else-if="winLabel === 'settings'" />
</template>
