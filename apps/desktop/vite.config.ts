import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// Tauri espera un puerto fijo y no debe ocultar los errores de Rust.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 5173, strictPort: true, host: '127.0.0.1' },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: { target: 'safari15', sourcemap: false },
  test: { environment: 'jsdom' },
});
