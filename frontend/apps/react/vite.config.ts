import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';
import { readFileSync } from 'node:fs';

const r = (p: string) => fileURLToPath(new URL(p, import.meta.url));

// Доступ снаружи (например, через обратный SSH-туннель при удалённой
// работе). По умолчанию ничего не меняется: dev-сервер как был доступен
// только с localhost, так и остаётся.
//   VITE_ALLOWED_HOSTS=203.0.113.10    — чей Host принимать (адрес для примера)
//   VITE_HMR_PORT=25565                — на какой порт стучится HMR
const allowedHosts = process.env.VITE_ALLOWED_HOSTS?.split(',').filter(Boolean);
const hmrPort = process.env.VITE_HMR_PORT ? Number(process.env.VITE_HMR_PORT) : undefined;
// Прогон проверок поднимает свой сервер на своей базе и своём порту
const apiTarget = process.env.VITE_API_TARGET ?? 'http://127.0.0.1:8080';
// HTTPS для стенда: микрофон браузер даёт только защищённой странице,
// и по http://адрес голосовой ввод недоступен совсем.
//   VITE_HTTPS_CERT=…/cert.pem VITE_HTTPS_KEY=…/key.pem
const httpsCert = process.env.VITE_HTTPS_CERT;
const httpsKey = process.env.VITE_HTTPS_KEY;
const https = httpsCert && httpsKey
  ? { cert: readFileSync(httpsCert), key: readFileSync(httpsKey) }
  : undefined;

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
    ...(allowedHosts ? { allowedHosts } : {}),
    ...(https ? { https } : {}),
    // Через туннель порт снаружи другой, и живая перезагрузка
    // стучалась бы не туда
    ...(hmrPort ? { hmr: { clientPort: hmrPort } } : {}),
    // Фронтенд и сервер живут на одном происхождении: так не нужен
    // CORS, а токен в заголовке уходит без предварительных CORS-запросов
    proxy: {
      '/api': { target: apiTarget, changeOrigin: true },
      // Канал класса: ws: true, иначе Vite отдаст обычный HTTP
      // и апгрейд соединения не произойдёт
      '/ws': { target: apiTarget.replace('http', 'ws'), ws: true },
    },
  },
  preview: { port: 4173, strictPort: true },
});
