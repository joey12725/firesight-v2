import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// Server-side tests: the functions in api/, the database layer and the dev tooling.
// The web app has its own suite (web/, `npm test --prefix web`).
export default defineConfig({
  // The web app's own path alias, so that a test can load one of its modules to check
  // that the server's copy of some logic still agrees with it (api/_tests/mcp-parity.test.ts).
  resolve: { alias: { '@': fileURLToPath(new URL('./web/src', import.meta.url)) } },
  test: {
    environment: 'node',
    include: ['api/**/*.test.ts', 'db/**/*.test.ts', 'scripts/**/*.test.ts'],
    // Each file boots an embedded Postgres and migrates it, and every sign-up hashes a
    // password: a second or two per file on an idle machine. The limits are generous so
    // that a machine busy with other builds does not turn slowness into failures.
    testTimeout: 120_000,
    hookTimeout: 240_000,
  },
});
