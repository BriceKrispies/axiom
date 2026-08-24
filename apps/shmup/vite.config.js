import { defineConfig } from 'vite';

export default defineConfig({
  // Bind IPv4 explicitly: the default `localhost` binds ::1 only on macOS,
  // which the capture harness (127.0.0.1) cannot reach.
  // `hmr: false` when the capture harness owns the server (OW_NO_HMR=1): a file
  // saved by a concurrently-working agent otherwise reloads the page mid-capture
  // and playwright fails with "Execution context was destroyed".
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    hmr: process.env.OW_NO_HMR ? false : undefined,
    // Unlocks the JS Self-Profiling API (`new Profiler(...)`), which is what
    // gives tools/bootprofile.mjs sampled, function-level attribution for the
    // parts of boot nobody hand-instrumented. Chrome refuses to construct a
    // Profiler without this header. Dev + preview only; it is a permission for
    // the page to profile ITSELF and carries no cross-origin exposure.
    headers: { 'Document-Policy': 'js-profiling' },
  },
  preview: { host: '127.0.0.1', headers: { 'Document-Policy': 'js-profiling' } },
  // RELATIVE asset URLs, so the built app runs from any path.
  //
  // Vite's default base is `/`, which emits `/assets/index-*.js` — correct only
  // when the app is the site root. The gallery serves every app from a
  // sub-directory (`/shmup/index.html`), and there the absolute path resolves to
  // the gallery root and 404s. `./` costs nothing at the root and makes the
  // build deployable anywhere, which is what every other app in the gallery
  // already assumes. Dev is unaffected; the dev server always serves from `/`.
  base: './',
  build: { target: 'es2022', sourcemap: true, chunkSizeWarningLimit: 4096 },
  // Large binary game assets served verbatim.
  assetsInclude: ['**/*.ktx2', '**/*.hdr', '**/*.exr', '**/*.bin', '**/*.glb'],
});
