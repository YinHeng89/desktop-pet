import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Tauri 开发环境配置：固定端口，避免与默认端口冲突
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5175,
    strictPort: true,
  },
  build: {
    target: 'esnext',
  },
})
