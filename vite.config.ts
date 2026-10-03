import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1431,
    strictPort: true,
    watch: {
      ignored: [
        '**/target/**',
        '**/crates/**',
        '**/src-tauri/**',
        '**/tests/**',
        '**/artifacts/**',
        '**/test-results/**',
        '**/docs/**',
      ],
    },
    host: '127.0.0.1',
    proxy: { '/api': { target: 'http://127.0.0.1:4319' } },
  },
  preview: { port: 1431, host: '127.0.0.1' },
  build: {
    chunkSizeWarningLimit: 800,
    rollupOptions: { output: { manualChunks: { charts: ['recharts'] } } },
  },
});
