import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

// Vitest 配置（PetBuddy）
//
// 说明：
// 1. 这里单独建 vitest.config.ts 而非复用 vite.config.ts，因为测试环境需要
//    jsdom + 覆盖率等与生产构建不同的配置；vue 插件需在此重新声明。
// 2. 未开启 `globals: true`：测试文件统一显式 `import { describe, it, expect } from 'vitest'`，
//    避免污染全局类型空间，也免去在 tsconfig 中加 vitest/globals 类型引用。
// 3. `passWithNoTests` 为重构过渡期开关：当前尚无测试用例，允许空套件通过。
//    Phase 9 补齐测试后应移除该项，让「误删测试文件」能被 CI 发现。
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.{test,spec}.ts'],
    // TODO(phase9): 补齐测试后移除，改为空套件即失败
    passWithNoTests: true,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'lcov'],
      reportsDirectory: './coverage',
      include: ['src/**/*.{ts,vue}'],
      exclude: [
        'src/test/**',
        'src/bindings/**',
        'src/**/*.spec.ts',
        'src/**/*.test.ts',
        'src/main.ts',
        'src/env.d.ts',
      ],
      // 覆盖率阈值在 Phase 9 补齐测试后启用，目标见 docs/refactor/TEST_PLAN.md §9
      // thresholds: { global: { lines: 60, functions: 60, branches: 55 } },
    },
  },
})
