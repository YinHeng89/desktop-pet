<script setup lang="ts">
// 测试夹具：用于验证「组件能被挂载」这一基础设施能力。
// 刻意做成真实 SFC（而非运行时 template），以复用 @vitejs/plugin-vue 的预编译，
// 避免为测试单独把 vue 别名指向完整构建（那会引入 __VUE_OPTIONS_API__ 等 define 依赖）。
defineProps<{ label: string }>()
</script>

<template>
  <span class="hello">{{ label }}</span>
</template>
