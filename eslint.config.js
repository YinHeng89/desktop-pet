// ESLint 9+ flat config（PetBuddy）
//
// 设计取舍：
// 1. 采用「非类型感知」规则集（不使用 parserOptions.project）。
//    类型相关规则（如 @typescript-eslint/no-floating-promises）由 vue-tsc 与
//    后续引入的 tsconfig.eslint.json 承担，避免让 lint 承担全量类型检查的成本与脆弱性。
//    → Phase 1 会补 tsconfig.eslint.json 并开启类型感知规则。
// 2. `no-undef` 关闭：ESLint 不做类型分析，无法识别 Tauri 注入的全局变量与
//    .d.ts 声明，开启会产生大量误报；该职责已由 vue-tsc 完全覆盖。
// 3. `npm run lint` 只让 error 失败（warn 不阻断）；`npm run lint:strict`
//    才启用 --max-warnings 0。这是为了让「架构分层守卫」类规则可以先以 warn
//    灰度观察，稳定后再转 error（见 docs/refactor/REFACTOR_PLAN.md 风险 RK8）。

import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import prettierConfig from 'eslint-config-prettier'

export default tseslint.config(
  {
    ignores: [
      'dist/**',
      'node_modules/**',
      'coverage/**',
      // Rust 侧由 cargo fmt / clippy 负责
      'src-tauri/**',
      // 官网静态页，独立维护（见 .prettierignore 同名条目）
      'website/**',
      // ts-rs 生成物（Phase 9 契约测试引入），不参与 lint
      'src/bindings/**',
    ],
  },

  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...pluginVue.configs['flat/recommended'],

  {
    // .vue 的 <script> 块交给 typescript-eslint 解析（vue-eslint-parser 作为外层 parser）
    files: ['**/*.vue'],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
        ecmaVersion: 'latest',
        sourceType: 'module',
      },
    },
  },

  {
    files: ['**/*.{ts,vue}'],
    rules: {
      // ── 与 TypeScript / vue-tsc 重复的规则 ──
      'no-undef': 'off',
      'no-unused-vars': 'off',

      // ── 通用正确性 ──
      'no-debugger': 'error',
      'no-var': 'error',
      'prefer-const': 'error',
      eqeqeq: ['error', 'smart'],
      // 项目有意使用 console.error 记录降级日志（Tauri 不可用时静默 degrade），不禁用
      'no-console': 'off',

      // ── TypeScript ──
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      '@typescript-eslint/no-explicit-any': 'warn',
      // 项目刻意使用 `let api: typeof import('@tauri-apps/api/window') | null` 这种
      // 写法来承载「按需动态 import 的模块」（见 src/tauri.ts 的 windowApi / coreApi）。
      // 这是惰性加载 Tauri API 的惯用模式，与「该用 type 修饰符」无关，故关闭本规则。
      '@typescript-eslint/consistent-type-imports': 'off',

      // ── Vue ──
      // App.vue / PetHost.vue 等单文件组件名是合法的，不强求多单词
      'vue/multi-word-component-names': 'off',
      'vue/no-v-html': 'error',
      // 其余 vue/* 排版类规则（html-indent、max-attributes-per-line、
      // singleline-html-element-content-newline 等）由末尾的 eslint-config-prettier
      // 统一关闭——排版交给 Prettier，避免两套工具互相打架。

      // ── 架构分层守卫 ──
      // eslint-plugin-import 及其 zones 将在 Phase 5（src/features/ 与 src/shared/
      // 目录落地后）启用，届时按 docs/refactor/REFACTOR_PLAN.md §1.3 的依赖规则配置：
      //   - features/*/model 仅可依赖 shared/config、shared/utils、shared/errors
      //   - shared/ 不得反向依赖 features/ 或 windows/
      //   - 业务层禁止直接 invoke，必须走 shared/ipc
      // 现阶段这些目录尚不存在，提前配置只会增加无谓的解析成本。
    },
  },

  {
    // 类型声明文件：Vue SFC 的官方 shim 必须写成
    // `DefineComponent<{}, {}, any>`，无法规避 `{}` 与 `any`，整体放宽。
    files: ['**/*.d.ts'],
    rules: {
      '@typescript-eslint/no-empty-object-type': 'off',
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },

  {
    // 测试文件放宽约束
    files: ['**/*.spec.ts', '**/*.test.ts', 'src/test/**/*.ts'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      'vue/multi-word-component-names': 'off',
    },
  },

  // ── 必须放最后 ──
  // eslint-config-prettier 关闭所有与 Prettier 冲突的排版规则。
  // 它只做「关闭」，不做「启用」，因此其后不能再有任何 rules 覆盖。
  prettierConfig,
)
