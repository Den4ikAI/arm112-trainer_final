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
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    // Фронтенд и сервер живут на одном происхождении: так не нужен
    // CORS, а токен в заголовке уходит без предварительных CORS-запросов
    proxy: {
      '/api': { target: 'http://127.0.0.1:8080', changeOrigin: true },
      // Канал класса: ws: true, иначе Vite отдаст обычный HTTP
      // и апгрейд соединения не произойдёт
      '/ws': { target: 'ws://127.0.0.1:8080', ws: true },
    },
  },
  preview: { port: 4173, strictPort: true },
});
