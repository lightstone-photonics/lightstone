import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  // Relative asset paths so the static build can be hosted under any sub-path.
  base: './',
  worker: { format: 'es' },
  server: {
    // Model documentation lives in ../docs and is imported as raw text.
    fs: { allow: ['..'] },
  },
});
