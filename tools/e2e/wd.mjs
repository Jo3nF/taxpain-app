// Cliente WebDriver mínimo para manejar TaxPain (Tauri + WebKitGTK) a través de tauri-driver.
import { mkdirSync, writeFileSync } from 'node:fs';

const BASE = `http://127.0.0.1:${process.env.WD_PORT ?? 4444}`;
const ELEM = 'element-6066-11e4-a6cc-4a5e8bd4b9f8';
export const SHOTS = process.env.SHOTS ?? 'e2e-shots';
mkdirSync(SHOTS, { recursive: true });

async function call(method, path, body) {
  const r = await fetch(BASE + path, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(`${method} ${path}: ${JSON.stringify(j.value ?? j).slice(0, 400)}`);
  return j.value;
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export class Session {
  static async start(app) {
    const v = await call('POST', '/session', {
      capabilities: { alwaysMatch: { 'tauri:options': { application: app } } },
    });
    const s = new Session();
    s.id = v.sessionId;
    return s;
  }
  p(x) {
    return `/session/${this.id}${x}`;
  }
  async quit() {
    await call('DELETE', `/session/${this.id}`).catch(() => {});
  }
  async inputs(xpath, n, ms = 15000) {
    const end = Date.now() + ms;
    for (;;) {
      const els = await this.findAll(xpath).catch(() => []);
      if (els.length >= n) return els;
      if (Date.now() > end) throw new Error(`Sólo ${els.length} de ${n}: ${xpath}`);
      await sleep(300);
    }
  }
  async findAll(xpath) {
    const v = await call('POST', this.p('/elements'), { using: 'xpath', value: xpath });
    return v.map((e) => e[ELEM] ?? Object.values(e)[0]);
  }
  async wait(xpath, ms = 20000) {
    const end = Date.now() + ms;
    for (;;) {
      const els = await this.findAll(xpath).catch(() => []);
      if (els.length) return els[0];
      if (Date.now() > end) {
        await this.shot('fallo-espera');
        throw new Error(`No aparece: ${xpath}`);
      }
      await sleep(300);
    }
  }
  /** Espera un elemento con ese texto visible (contiene, sin distinguir espacios). */
  text(t) {
    return `//*[contains(normalize-space(.), ${JSON.stringify(t)}) and not(.//*[contains(normalize-space(.), ${JSON.stringify(t)})])]`;
  }
  async waitText(t, ms) {
    return this.wait(this.text(t), ms);
  }
  async click(el) {
    await call('POST', this.p(`/element/${el}/click`), {});
  }
  async clickText(t, tag = '*') {
    const x = `//${tag}[contains(normalize-space(.), ${JSON.stringify(t)})][not(@disabled)]`;
    const els = await this.findAll(x);
    if (!els.length) {
      await this.shot('fallo-click');
      throw new Error(`No hay ${tag} con «${t}»`);
    }
    // el más interno
    await this.click(els[els.length - 1]);
  }
  async type(el, s) {
    await call('POST', this.p(`/element/${el}/clear`), {}).catch(() => {});
    await call('POST', this.p(`/element/${el}/value`), { text: s });
  }
  async exec(script, args = []) {
    return call('POST', this.p('/execute/sync'), { script, args });
  }
  async bodyText() {
    return this.exec('return document.body.innerText');
  }
  async shot(name) {
    const b64 = await call('GET', this.p('/screenshot')).catch(() => null);
    if (b64) writeFileSync(`${SHOTS}/${name}.png`, Buffer.from(b64, 'base64'));
  }
}
