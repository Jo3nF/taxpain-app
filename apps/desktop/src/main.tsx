import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { DataGate } from './DataProtection';
import './index.css';

const root = document.getElementById('root');
if (!root) throw new Error('Falta #root');

// Modo demo con datos sintéticos (capturas y vídeo). No existe dentro de la app de escritorio.
if (new URLSearchParams(window.location.search).has('demo') && !('__TAURI_INTERNALS__' in window)) {
  const { installDemo } = await import('./demo');
  installDemo();
}

createRoot(root).render(
  <StrictMode>
    <DataGate>
      <App />
    </DataGate>
  </StrictMode>,
);
