import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';

// Lints the server side: api/, db/ and scripts/. The web app has its own config (web/).
export default tseslint.config(
  { ignores: ['web/**', 'target/**', 'node_modules/**', '.local/**', 'api/_engine/**', 'crates/**', 'tools/**'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ['api/**/*.ts', 'db/**/*.ts', 'scripts/**/*.ts', '*.config.{js,ts}'],
    languageOptions: { globals: { ...globals.node } },
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
      '@typescript-eslint/consistent-type-imports': 'error',
    },
  },
  {
    // The functions write no logs at all, so that neither a credential nor a plan can
    // end up in one. The dev tooling in scripts/ may print.
    files: ['api/**/*.ts', 'db/**/*.ts'],
    rules: { 'no-console': 'error' },
  },
);
