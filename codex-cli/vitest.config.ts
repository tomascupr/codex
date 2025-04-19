import { defineConfig } from 'vitest/config';

// Vitest config for the codex‑cli package.
//
// When running under modern Node (≥22) the default worker‑pool implementation
// (provided by `tinypool`) currently crashes with a "Maximum call stack size
// exceeded" error during teardown.  Disabling the thread‑pool avoids the crash
// and does not measurably impact performance for this comparatively small test
// suite.
//
// See: https://github.com/tinylibs/tinypool/issues/194

export default defineConfig({
  test: {
    // Disable Vitest's worker pool; run all tests in the main thread.
    threads: false,
    // Force Vitest to exit even if there are stray timers or handles still
    // active. This avoids spurious non‑zero exit codes that otherwise occur
    // when some background `setTimeout` calls linger after the tests finish.
    exit: true,
  },
});
