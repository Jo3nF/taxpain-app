// Vigía del BOE: lee el sumario del día (API de datos abiertos del BOE) y lista las
// disposiciones que pueden afectar a TaxPain (modelos de la AEAT, VERI*FACTU, IVA, IRPF,
// Sociedades, plan contable, depósito de cuentas, fiestas laborales…). No cambia nada: el
// workflow `.github/workflows/boe.yml` abre una incidencia para revisarlo a mano.
// Idea del workflow de ingesta diaria de reforma-digital (MIT, docs/research/REFORMA_DIGITAL.md).
//
// Uso: node tools/boe/watch.mjs [AAAAMMDD]   (por omisión, hoy en Madrid)

const RULES = [
  [/\bmodelos? (?:\d{3}(?:[, y]+)?)+/i, 'modelos de la AEAT'],
  [/VERI\*?FACTU|sistemas informáticos de facturación|facturación electrónica/i, 'facturación'],
  [/Impuesto sobre el Valor Añadido|\bIVA\b/i, 'IVA'],
  [/Impuesto sobre la Renta de las Personas Físicas|\bIRPF\b/i, 'IRPF'],
  [/Impuesto sobre Sociedades/i, 'Sociedades'],
  [/Plan General de Contabilidad|Instituto de Contabilidad/i, 'contabilidad'],
  [/depósito de (?:las )?cuentas|Registro Mercantil|legalización de libros/i, 'Registro Mercantil'],
  [/fiestas laborales|días inhábiles/i, 'calendario'],
  [/Agencia Estatal de Administración Tributaria|Ley General Tributaria|Norma 43|SEPA/i, 'AEAT'],
];

// Ruido frecuente sin efecto en TaxPain.
const IGNORE = [/números de identificación fiscal/i, /nombramientos?|ceses?\b/i];

const asArray = (x) => (x === undefined ? [] : Array.isArray(x) ? x : [x]);

/** Disposiciones del sumario (secciones I y III) que casan con alguna regla. */
export function matches(sumario) {
  const out = [];
  for (const diario of asArray(sumario?.data?.sumario?.diario)) {
    for (const sec of asArray(diario.seccion)) {
      if (!['1', '3'].includes(String(sec.codigo))) continue;
      for (const dep of asArray(sec.departamento)) {
        const items = [
          ...asArray(dep.item),
          ...asArray(dep.epigrafe).flatMap((e) => asArray(e.item)),
        ];
        for (const it of items) {
          const title = String(it.titulo ?? '');
          if (IGNORE.some((re) => re.test(title))) continue;
          const topics = RULES.filter(([re]) => re.test(title)).map(([, t]) => t);
          if (topics.length === 0) continue;
          out.push({
            id: it.identificador,
            department: dep.nombre,
            title,
            topics: [...new Set(topics)],
            url: it.url_html ?? `https://www.boe.es/diario_boe/txt.php?id=${it.identificador}`,
          });
        }
      }
    }
  }
  return out;
}

export function markdown(date, found) {
  if (found.length === 0) return '';
  const lines = [`Disposiciones del BOE del ${date} que pueden afectar a TaxPain:`, ''];
  for (const f of found) {
    lines.push(`- [${f.id}](${f.url}) · ${f.department} · _${f.topics.join(', ')}_  `);
    lines.push(`  ${f.title}`);
  }
  lines.push('', 'Revisar a mano y, si cambia algo, actualizar el código o los `TODO(VERIFY)`.');
  return lines.join('\n');
}

async function main() {
  const date =
    process.argv[2] ??
    new Intl.DateTimeFormat('sv-SE', { timeZone: 'Europe/Madrid' })
      .format(new Date())
      .replaceAll('-', '');
  const res = await fetch(`https://www.boe.es/datosabiertos/api/boe/sumario/${date}`, {
    headers: { Accept: 'application/json' },
  });
  if (res.status === 404) return; // domingo o festivo: no hay BOE
  if (!res.ok) throw new Error(`BOE ${date}: HTTP ${res.status}`);
  process.stdout.write(markdown(date, matches(await res.json())));
}

if (process.argv[1]?.endsWith('watch.mjs')) await main();
