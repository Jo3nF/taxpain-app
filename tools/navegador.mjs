// Abre el navegador para las herramientas (accesibilidad, capturas, vídeos).
// Usa el Chromium de Playwright si está instalado y, si no, el Google Chrome del sistema,
// para no tener que descargar nada.
import { chromium } from 'playwright-core';

export async function abrir(opciones = {}) {
  try {
    return await chromium.launch(opciones);
  } catch (error) {
    try {
      return await chromium.launch({ ...opciones, channel: 'chrome' });
    } catch {
      throw error;
    }
  }
}
