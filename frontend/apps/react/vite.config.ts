import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';

const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      // Слои FSD внутри приложения.
      '@': r('./src'),
      // Общие токены и примеси — один источник правды для React и Angular.
      '@styles': r('../../packages/styles'),
      // Доменный пакет: типы, классификатор, машина статусов, оценка.
      '@arm112/domain': r('../../packages/domain/src/index.ts'),
    },
  },
  server: { port: 5173, strictPort: true },
  preview: { port: 4173, strictPort: true },
});
