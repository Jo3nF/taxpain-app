import { useCallback, useEffect, useRef, useState } from 'react';
import { AskAiButton, useAskAi } from './AssistantPanel';
import {
  type AeatPresented,
  aeatPresented,
  type CertificateInfo,
  draftPeriod,
  draftTitle,
  draftTopic,
  errorMessage,
  euros,
  type FiledReturn,
  type FilingResult,
  filedReceiptDownload,
  filedReceiptSave,
  filedReturns,
  formatDate,
  getProfile,
  type IrpfRegime,
  listCertificates,
  MENSUALES,
  MES,
  MESES,
  type ModelDraft,
  type ModelSettings,
  modelDraft,
  modelExport,
  modelFileTesting,
  modelMonthly,
  modelPrevalidate,
  modelSettings,
  PRESENTABLE,
  PREVALIDATABLE,
  type PrevalidationResult,
  parseEuros,
  pickFolder,
  RECTIFICABLES,
  type Rectificable,
  type RectificativaAnterior,
  type Reduction32,
  rectificativaAnterior,
  saveModelSettings,
  sha256Hex,
  type TaxpayerProfile,
  type YearSettings,
} from './api';
import { BienesExtranjero } from './BienesExtranjero';
import { Certificados } from './Certificados';
import { CertificateSelect, certLabel } from './CertificateSelect';
import { YearStepper } from './Controls';
import { useInlineDialog, useReturnFocus } from './dialogs';
import { Entity184 } from './Entity184';
import { arrendadoresDe, InmueblesArrendados } from './InmueblesArrendados';
import { Letters347 } from './Letters347';
import { ModelAdjust } from './ModelAdjust';
import { Modelo200 } from './Modelo200';
import { NoResidentes } from './NoResidentes';
import { Pago202 } from './Pago202';
import { PresentadoAeat } from './PresentadoAeat';
import { anteriorTexto, RectificativaPanel } from './Rectificativa';
import { RentaActivity } from './RentaActivity';
import { RentaCheck } from './RentaCheck';
import { SiiPanel } from './Sii';
import { TaxPaymentPanel } from './TaxPayment';

/** Nombre corto de cada casilla (instrucciones oficiales de cada modelo). */
export const BOX_LABELS: Record<string, Record<number, string>> = {
  '303': {
    1: 'Base al 4 %',
    3: 'Cuota al 4 %',
    4: 'Base al 10 %',
    6: 'Cuota al 10 %',
    7: 'Base al 21 %',
    9: 'Cuota al 21 %',
    14: 'Modificación de bases (rectificativas)',
    15: 'Modificación de cuotas (rectificativas)',
    156: 'Recargo de equivalencia 1,75 %: base',
    158: 'Recargo de equivalencia 1,75 %: cuota',
    168: 'Recargo de equivalencia 0,5 %: base',
    170: 'Recargo de equivalencia 0,5 %: cuota',
    19: 'Recargo de equivalencia 1,4 %: base',
    21: 'Recargo de equivalencia 1,4 %: cuota',
    22: 'Recargo de equivalencia 5,2 %: base',
    24: 'Recargo de equivalencia 5,2 %: cuota',
    25: 'Modificación de recargo: base',
    26: 'Modificación de recargo: cuota',
    27: 'Total cuota devengada',
    28: 'Deducible, operaciones corrientes: base',
    29: 'Deducible, operaciones corrientes: cuota',
    30: 'Deducible, bienes de inversión: base',
    31: 'Deducible, bienes de inversión: cuota',
    40: 'Rectificación de deducciones: base',
    41: 'Rectificación de deducciones: cuota',
    45: 'Total a deducir',
    46: 'Resultado régimen general',
    59: 'Entregas intracomunitarias',
    60: 'Exportaciones',
    120: 'No sujetas por reglas de localización',
    122: 'Con inversión del sujeto pasivo',
    64: 'Suma de resultados',
    65: '% atribuible al Estado',
    66: 'Atribuible al Estado',
    110: 'Cuotas a compensar de periodos anteriores',
    78: 'Compensadas en este periodo',
    87: 'Pendientes para periodos posteriores',
    69: 'Resultado de la autoliquidación',
    71: 'Resultado',
    // Página 04 del 4T (exonerados del 390): volumen de operaciones del año.
    80: 'Volumen anual: régimen general',
    81: 'Volumen anual: criterio de caja (art. 75)',
    93: 'Volumen anual: entregas intracomunitarias',
    94: 'Volumen anual: exportaciones y exentas con deducción',
    83: 'Volumen anual: exentas sin deducción',
    84: 'Volumen anual: no sujetas por localización',
    125: 'Volumen anual: inversión del sujeto pasivo',
    79: 'Volumen anual: entregas de inmuebles no habituales (se restan)',
    99: 'Volumen anual: entregas de bienes de inversión (se restan)',
    88: 'Volumen anual de operaciones (total)',
  },
  '130': {
    1: 'Ingresos computables (acumulado)',
    2: 'Gastos deducibles (acumulado)',
    3: 'Rendimiento neto',
    4: '20 % del rendimiento',
    5: 'Pagos fraccionados de trimestres anteriores',
    6: 'Retenciones e ingresos a cuenta',
    7: 'Pago fraccionado previo',
    12: 'Suma de pagos fraccionados',
    13: 'Minoración (art. 110.3.c RIRPF)',
    14: 'Diferencia',
    15: 'Resultados negativos anteriores',
    17: 'Total',
    19: 'Resultado',
  },
  '111': {
    1: 'Trabajo: nº de perceptores',
    2: 'Trabajo: percepciones',
    3: 'Trabajo: retenciones',
    7: 'Actividades económicas: nº de perceptores',
    8: 'Actividades económicas: percepciones',
    9: 'Actividades económicas: retenciones',
    28: 'Suma de retenciones',
    30: 'Resultado a ingresar',
  },
  '390': {
    1: 'Devengado 4 %: base',
    2: 'Devengado 4 %: cuota',
    3: 'Devengado 10 %: base',
    4: 'Devengado 10 %: cuota',
    5: 'Devengado 21 %: base',
    6: 'Devengado 21 %: cuota',
    29: 'Modificación de bases',
    30: 'Modificación de cuotas',
    33: 'Total bases de IVA',
    34: 'Total cuotas de IVA',
    35: 'Recargo 0,5 %: base',
    36: 'Recargo 0,5 %: cuota',
    599: 'Recargo 1,4 %: base',
    600: 'Recargo 1,4 %: cuota',
    601: 'Recargo 5,2 %: base',
    602: 'Recargo 5,2 %: cuota',
    41: 'Recargo 1,75 %: base',
    42: 'Recargo 1,75 %: cuota',
    47: 'Total cuotas de IVA y recargo',
    190: 'Deducible corriente 4 %: base',
    191: 'Deducible corriente 4 %: cuota',
    603: 'Deducible corriente 10 %: base',
    604: 'Deducible corriente 10 %: cuota',
    605: 'Deducible corriente 21 %: base',
    606: 'Deducible corriente 21 %: cuota',
    48: 'Total deducible corriente: base',
    49: 'Total deducible corriente: cuota',
    196: 'Bienes de inversión 4 %: base',
    197: 'Bienes de inversión 4 %: cuota',
    611: 'Bienes de inversión 10 %: base',
    612: 'Bienes de inversión 10 %: cuota',
    613: 'Bienes de inversión 21 %: base',
    614: 'Bienes de inversión 21 %: cuota',
    50: 'Total bienes de inversión: base',
    51: 'Total bienes de inversión: cuota',
    639: 'Rectificación de deducciones: base',
    62: 'Rectificación de deducciones: cuota',
    64: 'Suma de deducciones',
    65: 'Resultado régimen general',
    84: 'Suma de resultados',
    85: 'Compensación de cuotas del ejercicio anterior',
    86: 'Resultado de la liquidación',
    95: 'Total ingresado en los 303 del año',
    97: 'Último periodo: a compensar',
    98: 'Último periodo: a devolver',
    99: 'Volumen: régimen general',
    103: 'Volumen: entregas intracomunitarias',
    104: 'Volumen: exportaciones',
    105: 'Volumen: exentas sin derecho a deducción',
    110: 'Volumen: no sujetas por localización',
    125: 'Volumen: inversión del sujeto pasivo',
    106: 'Volumen: entregas de inmuebles no habituales (se restan)',
    107: 'Volumen: entregas de bienes de inversión (se restan)',
    108: 'Total volumen de operaciones',
  },
  '202': {
    1: 'Base del pago fraccionado (art. 40.2 LIS)',
    3: 'A ingresar (18 %)',
    4: 'Resultado contable del periodo (art. 40.3 LIS)',
    5: 'Corrección por IS: aumentos',
    6: 'Corrección por IS: disminuciones',
    7: 'Otras correcciones: aumentos',
    8: 'Otras correcciones: disminuciones',
    38: 'Total aumentos',
    39: 'Total disminuciones',
    13: 'Base imponible previa',
    44: 'Remanente de la reserva de capitalización',
    14: 'Compensación de bases negativas',
    16: 'Base del pago fraccionado',
    17: 'Porcentaje (%)',
    18: 'Resultado previo',
    19: 'Base del pago fraccionado (dos porcentajes)',
    20: 'Base al primer porcentaje',
    21: 'Primer porcentaje (%)',
    22: 'Pago al primer porcentaje',
    23: 'Base al segundo porcentaje',
    24: 'Segundo porcentaje (%)',
    25: 'Pago al segundo porcentaje',
    26: 'Resultado previo',
    27: 'Bonificaciones',
    28: 'Retenciones e ingresos a cuenta del periodo',
    29: 'Volumen de operaciones en territorio común (%)',
    30: 'Pagos fraccionados anteriores del ejercicio',
    32: 'Resultado',
    33: 'Pago mínimo (10 M€ o más)',
    34: 'A ingresar',
  },
  '200': {
    500: 'Resultado de la cuenta de pérdidas y ganancias',
    301: 'Corrección por IS: aumentos',
    302: 'Corrección por IS: disminuciones',
    501: 'Resultado antes del IS',
    417: 'Otras correcciones: aumentos',
    418: 'Otras correcciones: disminuciones',
    550: 'Base imponible antes de compensar bases negativas',
    1032: 'Reserva de capitalización',
    547: 'Compensación de bases negativas',
    552: 'Base imponible',
    558: 'Tipo de gravamen (%)',
    562: 'Cuota íntegra',
    582: 'Cuota íntegra ajustada positiva',
    592: 'Cuota líquida',
    1766: 'Retenciones e ingresos a cuenta',
    599: 'Cuota del ejercicio a ingresar o devolver',
    601: '1.er pago fraccionado',
    603: '2.º pago fraccionado',
    605: '3.er pago fraccionado',
    611: 'Cuota diferencial',
    1586: 'Resultado de la autoliquidación',
  },
  '115': {
    1: 'Nº de perceptores',
    2: 'Base de las retenciones',
    3: 'Retenciones',
    5: 'Resultado a ingresar',
  },
  '216': {
    1: 'Rentas sometidas a retención: nº de rentas',
    2: 'Rentas sometidas a retención: base',
    3: 'Retenciones e ingresos a cuenta',
    4: 'Rentas no sometidas a retención: nº de rentas',
    5: 'Rentas no sometidas a retención: base',
    6: 'A deducir (complementaria)',
    7: 'Resultado a ingresar',
  },
  '123': {
    1: 'Dividendos: nº de rentas',
    2: 'Resto de rentas: nº de rentas',
    3: 'Total: nº de rentas',
    4: 'Dividendos: base de las retenciones',
    5: 'Resto de rentas: base de las retenciones',
    6: 'Total: base de las retenciones',
    7: 'Dividendos: retenciones',
    8: 'Resto de rentas: retenciones',
    9: 'Total de retenciones',
    10: 'Periodificación: ingresos de ejercicios anteriores',
    11: 'Periodificación: regularización',
    12: 'Retenciones y regularización',
    13: 'A deducir (complementaria)',
    14: 'Resultado a ingresar',
  },
};

/** Casillas que son porcentajes con dos decimales, no importes. */
const PERCENT: Record<string, number[]> = {
  '303': [65],
  '200': [558],
  '202': [17, 21, 24, 29],
};

/**
 * Tipos del IS por ejercicio (art. 29.1 LIS y DT 44.ª, redacción de la Ley 7/2024): espejo de
 * `small_company_rates` en crates/models/src/m200.rs, que es quien calcula el 200. `micro` son los
 * dos tramos (hasta 50.000 € y el resto) de las entidades con cifra de negocios inferior a 1 M€;
 * `reduced`, el tipo único del resto de reducida dimensión; la nueva creación tributa al 15 % sobre
 * toda la base.
 */
export function isRates(fy: number): {
  general: number;
  reduced: number;
  micro: [number, number];
  startup: number;
} {
  const [micro, reduced]: [[number, number], number] =
    fy <= 2022
      ? [[25, 25], 25]
      : fy <= 2024
        ? [[23, 23], 25]
        : fy === 2025
          ? [[21, 22], 24]
          : fy === 2026
            ? [[19, 21], 23]
            : fy === 2027
              ? [[17, 20], 22]
              : fy === 2028
                ? [[17, 20], 21]
                : [[17, 20], 20];
  return { general: 25, reduced, micro, startup: 15 };
}

type Regime = NonNullable<ModelSettings['corporate_regime']>;

/** Etiquetas de los regímenes del IS con los tipos del ejercicio; sin año, sin los que cambian. */
export function regimeLabels(fy?: number): Record<Regime, string> {
  const r = isRates(fy ?? 0);
  const micro =
    fy === undefined
      ? 'según el ejercicio'
      : r.micro[0] === r.micro[1]
        ? `${r.micro[0]} %`
        : `${r.micro[0]} % hasta 50.000 €, ${r.micro[1]} % el resto`;
  return {
    General: `General (${r.general} %)`,
    ReducidaDimension: `Reducida dimensión (${fy === undefined ? 'según el ejercicio' : `${r.reduced} % en ${fy}`})`,
    Micro: `Cifra de negocios inferior a 1 millón (${micro})`,
    NuevaCreacion: `Nueva creación (${r.startup} %)`,
  };
}

const REGIMES: Regime[] = ['General', 'ReducidaDimension', 'Micro', 'NuevaCreacion'];

function percent(hundredths: number): string {
  return `${euros(hundredths).replace(' €', '')} %`;
}

const DECLARATION: Record<string, string> = {
  I: 'A ingresar',
  U: 'A ingresar por domiciliación',
  C: 'A compensar en los próximos trimestres',
  D: 'A devolver',
  N: 'Sin actividad o resultado cero',
  B: 'Negativo: a deducir en los próximos pagos fraccionados',
};

/** Qué es el importe destacado en los modelos informativos y en el 390. */
const SUMMARY: Record<string, string> = {
  '349': 'Total de operaciones intracomunitarias',
  '390': 'Resultado de la liquidación anual',
  '200': 'Resultado: a ingresar si es positivo, a devolver si es negativo',
  '347': 'Total de operaciones con terceros',
  '190': 'Total de retenciones',
  '180': 'Total de retenciones',
  '184': 'Rendimiento neto de la entidad (se atribuye a los socios)',
  '193': 'Total de retenciones sobre dividendos',
  '296': 'Total de retenciones a no residentes',
};

const MODELS: { id: string; label: string; annual?: boolean; periods?: string[] }[] = [
  { id: '303', label: '303 · IVA' },
  { id: '130', label: '130 · IRPF pago fraccionado' },
  { id: '111', label: '111 · Retenciones' },
  { id: '115', label: '115 · Retenciones de alquileres' },
  { id: '123', label: '123 · Retenciones de dividendos' },
  { id: '216', label: '216 · Retenciones a no residentes' },
  { id: '349', label: '349 · Operaciones intracomunitarias' },
  { id: '390', label: '390 · Resumen anual de IVA', annual: true },
  { id: '202', label: '202 · Pago fraccionado de Sociedades', periods: ['1P', '2P', '3P'] },
  { id: '200', label: '200 · Impuesto sobre Sociedades (borrador)', annual: true },
  { id: '347', label: '347 · Operaciones con terceros', annual: true },
  { id: '190', label: '190 · Resumen anual de retenciones', annual: true },
  { id: '180', label: '180 · Resumen anual de alquileres', annual: true },
  { id: '193', label: '193 · Resumen anual de dividendos', annual: true },
  { id: '296', label: '296 · Resumen anual de no residentes', annual: true },
  { id: '184', label: '184 · Entidades en atribución de rentas', annual: true },
  { id: '720', label: '720 · Bienes en el extranjero (borrador)', annual: true },
  { id: '721', label: '721 · Monedas virtuales en el extranjero (borrador)', annual: true },
];

/** Modelos con su propio borrador (sin fichero: TaxPain no tiene su diseño de registro). */
const EXTRANJERO = ['720', '721'];

function currentQuarter(d = new Date()): { year: number; quarter: number } {
  // El trimestre que toca presentar es el anterior al actual.
  const q = Math.floor(d.getMonth() / 3);
  return q === 0
    ? { year: d.getFullYear() - 1, quarter: 4 }
    : { year: d.getFullYear(), quarter: q };
}

export function DraftView({
  d,
  questions = 0,
  onAsk,
}: {
  d: ModelDraft;
  /** Preguntas de su conversación con la IA (si la hay). */
  questions?: number;
  /** Abre la conversación con la IA sobre este borrador. */
  onAsk?: (from: HTMLButtonElement) => void;
}) {
  const labels = BOX_LABELS[d.model] ?? {};
  const rows: [number, string][] = [
    ...d.payees.map(([k, n]): [number, string] => [k, String(n)]),
    ...d.boxes.map(([k, v]): [number, string] => [
      k,
      (PERCENT[d.model] ?? []).includes(k) ? percent(v) : euros(v),
    ]),
  ];
  return (
    <div className="mt-4 space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3 rounded-lg border border-stone-200 bg-white p-4">
        <div>
          <p className="text-sm text-stone-600">
            Modelo {d.model} · {draftPeriod(d.model, d.year, d.quarter)}
          </p>
          <p className="mt-1 text-2xl font-semibold tabular-nums">{euros(d.result)}</p>
          <p className="text-sm">
            {SUMMARY[d.model] ?? DECLARATION[d.declaration] ?? d.declaration}
          </p>
        </div>
        {onAsk && (
          <AskAiButton
            what={`modelo ${d.model} · ${draftPeriod(d.model, d.year, d.quarter)}`}
            about="sobre este borrador"
            questions={questions}
            onOpen={onAsk}
          />
        )}
      </div>
      {d.warnings.length > 0 && (
        <ul role="alert" className="list-disc space-y-1 pl-5 text-sm text-amber-800">
          {d.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      {d.details.length > 0 && (
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-stone-500">
              <th className="py-1">
                {d.model === '349'
                  ? 'Operador'
                  : d.model === '390'
                    ? 'Venta de inmovilizado'
                    : 'Tercero'}
              </th>
              <th className="text-right">
                {d.model === '349'
                  ? 'Base imponible'
                  : d.model === '347'
                    ? 'Importe anual'
                    : d.model === '390'
                      ? 'Se resta del volumen'
                      : 'Retenciones'}
              </th>
            </tr>
          </thead>
          <tbody>
            {d.details.map(([label, v]) => (
              <tr key={label} className="border-t border-stone-100">
                <td className="py-1">{label}</td>
                <td className="text-right tabular-nums">{euros(v)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {rows.length > 0 && (
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-stone-500">
              <th className="w-16 py-1">Casilla</th>
              <th>Concepto</th>
              <th className="text-right">Importe</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(([k, v]) => (
              <tr key={`${k}-${v}`} className="border-t border-stone-100">
                <td className="py-1 tabular-nums">[{String(k).padStart(2, '0')}]</td>
                <td>{labels[k] ?? ''}</td>
                <td className="text-right tabular-nums">{v}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/** Presentación en el entorno de pruebas, con conformidad expresa del usuario. */
function FileTesting({
  draft,
  onClose,
  onFiled,
}: {
  draft: ModelDraft;
  onClose: () => void;
  onFiled: () => void;
}) {
  const [certs, setCerts] = useState<CertificateInfo[]>([]);
  const [cert, setCert] = useState('');
  const [agree, setAgree] = useState(false);
  const [nrc, setNrc] = useState('');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<FilingResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [inDoubt, setInDoubt] = useState(false);
  const [retry, setRetry] = useState(false);
  const [hash, setHash] = useState('');
  const [ready, setReady] = useState(false);
  const dialog = useInlineDialog(ready, busy, onClose);
  useEffect(() => {
    sha256Hex(draft.file)
      .then(setHash)
      .catch(() => setHash(''));
  }, [draft.file]);
  useEffect(() => {
    listCertificates()
      .then((c) => {
        setCerts(c);
        setCert(c[0]?.fingerprint_sha256 ?? '');
      })
      .catch(() => setCerts([]))
      .finally(() => setReady(true));
  }, []);
  // Si el borrador cambia (datos nuevos o ajustes guardados), la conformidad ya no vale.
  // biome-ignore lint/correctness/useExhaustiveDependencies: se reinicia al cambiar el fichero
  useEffect(() => {
    setAgree(false);
    setResult(null);
  }, [draft.file]);
  const needsNrc = draft.declaration === 'I';
  const filed = result?.accepted === true;
  return (
    <div
      ref={dialog.ref}
      role="dialog"
      aria-label="Presentar en el entorno de pruebas"
      className="mt-4 space-y-3 rounded-lg border border-red-300 bg-red-50 p-4 text-sm"
      onKeyDown={dialog.onKeyDown}
    >
      <p className="font-medium">Presentar en el entorno de pruebas de la AEAT</p>
      <p>
        Se presentará el modelo {draft.model} con el resultado de{' '}
        <strong>{euros(draft.result)}</strong> en el <strong>entorno de pruebas</strong>{' '}
        (prewww1.aeat.es, días laborables de 8:00 a 15:00). No tiene efectos reales, pero funciona
        como la presentación de verdad: enviarla supone aceptar los datos del borrador.
      </p>
      {hash && (
        <p className="text-xs text-stone-600">
          Se envía exactamente este fichero ({draft.file_name}), con huella SHA-256{' '}
          <code className="break-all font-mono" title={hash}>
            {hash.match(/.{1,8}/g)?.join(' ')}
          </code>
          . La misma huella queda en la auditoría y en el justificante guardado.
        </p>
      )}
      {certs.length === 0 ? (
        <p role="alert">Importa primero tu certificado en Ajustes.</p>
      ) : (
        <label className="block max-w-md">
          Certificado
          <select
            className="mt-1 block w-full"
            value={cert}
            onChange={(e) => setCert(e.target.value)}
          >
            {certs.map((c) => (
              <option key={c.fingerprint_sha256} value={c.fingerprint_sha256}>
                {certLabel(c)}
              </option>
            ))}
          </select>
        </label>
      )}
      {needsNrc && (
        <label className="block max-w-md">
          NRC del pago (te lo da tu banco al pagar)
          <input
            className="mt-1 block w-full font-mono"
            value={nrc}
            onChange={(e) => setNrc(e.target.value.trim())}
            maxLength={22}
          />
        </label>
      )}
      <label className="flex items-start gap-2">
        <input type="checkbox" checked={agree} onChange={(e) => setAgree(e.target.checked)} />
        He revisado el borrador y estoy conforme con sus datos
      </label>
      {inDoubt && (
        <label className="flex items-start gap-2">
          <input type="checkbox" checked={retry} onChange={(e) => setRetry(e.target.checked)} />
          He consultado en la AEAT y la declaración no consta: volver a presentarla
        </label>
      )}
      <div className="flex gap-2">
        <button
          type="button"
          disabled={
            !cert ||
            !agree ||
            busy ||
            filed ||
            (needsNrc && nrc.length !== 22) ||
            (inDoubt && !retry)
          }
          className="rounded btn-danger px-3 py-1.5 disabled:opacity-50"
          onClick={async () => {
            setError(null);
            setBusy(true);
            try {
              const r = await modelFileTesting(
                draft.model,
                draft.year,
                draft.quarter,
                cert,
                needsNrc ? nrc : null,
                draft.file,
                retry,
              );
              setResult(r);
              setInDoubt(false);
              setRetry(false);
              if (r.accepted) onFiled();
            } catch (e) {
              const m = errorMessage(e);
              setError(m);
              if (/no tuvo respuesta|No se sabe si la AEAT/.test(m)) setInDoubt(true);
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? 'Presentando…' : 'Presentar (pruebas)'}
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1.5"
          onClick={onClose}
        >
          Cerrar
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {result && !result.accepted && (
        <div role="alert">
          <p className="font-medium">La AEAT la ha rechazado:</p>
          <ul className="list-disc pl-5">
            {result.errors.map((m) => (
              <li key={m}>{m}</li>
            ))}
          </ul>
        </div>
      )}
      {result?.receipt && (
        <div role="status" className="space-y-1">
          <p className="font-medium">Presentada en el entorno de pruebas.</p>
          <p>
            CSV {result.receipt.csv} · justificante {result.receipt.receipt_number}
          </p>
          <p className="break-all">Justificante en PDF: {result.receipt.pdf_url}</p>
          {[...result.receipt.notices, ...result.receipt.warnings].map((m) => (
            <p key={m}>{m}</p>
          ))}
        </div>
      )}
    </div>
  );
}

/** Consulta de lo presentado según la propia AEAT (también lo presentado fuera de TaxPain). */
function AeatQuery({ model, year }: { model: string; year: number }) {
  const [list, setList] = useState<AeatPresented[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [cert, setCert] = useState('');
  return (
    <div className="mt-4 flex flex-wrap items-center gap-3 text-sm">
      <button
        type="button"
        disabled={busy}
        className="rounded border border-stone-300 px-3 py-1"
        onClick={async () => {
          setError(null);
          if (!cert) {
            setError('Importa primero tu certificado en Ajustes.');
            return;
          }
          setBusy(true);
          try {
            setList(await aeatPresented(model, year, cert));
          } catch (e) {
            setError(errorMessage(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy ? 'Consultando…' : `Consultar en la AEAT los ${model} de ${year}`}
      </button>
      <CertificateSelect value={cert} onChange={setCert} />
      {error && (
        <p role="alert" className="mt-2 w-full text-red-700">
          {error}
        </p>
      )}
      {list && (
        <ul className="mt-2 w-full space-y-1">
          {list.length === 0 && <li>La AEAT no tiene declaraciones de ese modelo y año.</li>}
          {list.map((p) => (
            <li key={`${p.model}-${p.period}-${p.receipt_number}`}>
              {p.model} · {p.period} · justificante {p.receipt_number}
              {p.previous_receipt ? ` (sustituye a ${p.previous_receipt})` : ''} ·{' '}
              {formatDate(p.presented_at.slice(0, 10))}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function FiledList({ year, version }: { year: number; version: number }) {
  const [list, setList] = useState<FiledReturn[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  // La presentación no guarda con qué certificado se hizo: para descargar el justificante se elige.
  const [cert, setCert] = useState('');
  useEffect(() => {
    void version;
    void reload;
    setLoadError(null);
    filedReturns(year)
      .then(setList)
      .catch((e) => {
        setList([]);
        setLoadError(errorMessage(e));
      });
  }, [year, version, reload]);
  const receipt = async (r: FiledReturn) => {
    setMsg(null);
    setError(null);
    try {
      if (!r.has_receipt_pdf) {
        if (!cert) throw { message: 'Importa primero tu certificado en Ajustes.' };
        await filedReceiptDownload(r.id, cert);
        setReload((n) => n + 1);
      }
      const dir = await pickFolder();
      if (dir) setMsg(`Justificante guardado en ${await filedReceiptSave(r.id, dir)}`);
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  if (loadError) {
    return (
      <p role="alert" className="mt-8 text-sm text-red-700">
        No se pudieron leer las presentaciones de {year}: {loadError}
      </p>
    );
  }
  if (list.length === 0) return null;
  return (
    <section aria-labelledby="presentadas-h" className="mt-8">
      <h3 id="presentadas-h" className="font-medium">
        Presentadas desde TaxPain en {year}
      </h3>
      {list.some((r) => !r.has_receipt_pdf) && (
        <div className="mt-2">
          <CertificateSelect value={cert} onChange={setCert} />
        </div>
      )}
      <table className="mt-2 w-full text-sm">
        <tbody>
          {list.map((r) => (
            <tr key={r.id} className="border-t border-stone-100">
              <td className="py-1">
                {r.model} · {r.period}
              </td>
              <td>{r.environment === 'pruebas' ? 'Pruebas' : 'Real'}</td>
              <td className="tabular-nums">{euros(r.result)}</td>
              <td>CSV {r.csv}</td>
              <td>{formatDate(r.presented_at.slice(0, 10))}</td>
              <td>
                <button type="button" className="text-link" onClick={() => receipt(r)}>
                  {r.has_receipt_pdf ? 'Guardar justificante…' : 'Descargar justificante…'}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {msg && (
        <p role="status" className="mt-2 text-sm">
          {msg}
        </p>
      )}
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-700">
          {error}
        </p>
      )}
    </section>
  );
}

/** Confirmación y resultado de la prevalidación con la AEAT (sin presentar nada). */
function Prevalidate({
  model,
  year,
  quarter,
  onClose,
}: {
  model: string;
  year: number;
  quarter: number;
  onClose: () => void;
}) {
  const [certs, setCerts] = useState<CertificateInfo[]>([]);
  const [cert, setCert] = useState('');
  const [savePdf, setSavePdf] = useState(true);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<PrevalidationResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const dialog = useInlineDialog(ready, busy, onClose);
  useEffect(() => {
    listCertificates()
      .then((c) => {
        setCerts(c);
        setCert(c[0]?.fingerprint_sha256 ?? '');
      })
      .catch(() => setCerts([]))
      .finally(() => setReady(true));
  }, []);
  return (
    <div
      ref={dialog.ref}
      role="dialog"
      aria-label="Prevalidar con la AEAT"
      className="mt-4 space-y-3 rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm"
      onKeyDown={dialog.onKeyDown}
    >
      <p className="font-medium">Prevalidar con la AEAT</p>
      <p>
        Se enviará el fichero de este borrador al servicio de validación de la Agencia Tributaria
        (ServValiDos). <strong>No se presenta nada</strong> ni tiene efectos, pero los datos de la
        declaración salen de tu Mac. TaxPain usará tu certificado y te pedirá Touch ID.
      </p>
      {certs.length === 0 ? (
        <p role="alert">Importa primero tu certificado en Ajustes.</p>
      ) : (
        <label className="block max-w-md">
          Certificado
          <select
            className="mt-1 block w-full"
            value={cert}
            onChange={(e) => setCert(e.target.value)}
          >
            {certs.map((c) => (
              <option key={c.fingerprint_sha256} value={c.fingerprint_sha256}>
                {certLabel(c)}
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="flex items-start gap-2">
        <input type="checkbox" checked={savePdf} onChange={(e) => setSavePdf(e.target.checked)} />
        Guardar el borrador oficial en PDF
      </label>
      <div className="flex gap-2">
        <button
          type="button"
          disabled={!cert || busy}
          className="rounded btn-primary px-3 py-1.5 disabled:opacity-50"
          onClick={async () => {
            setError(null);
            const dir = savePdf ? await pickFolder() : null;
            if (savePdf && !dir) return;
            setBusy(true);
            try {
              setResult(await modelPrevalidate(model, year, quarter, cert, dir));
            } catch (e) {
              setError(errorMessage(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? 'Enviando…' : 'Sí, prevalidar'}
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1.5"
          onClick={onClose}
        >
          Cerrar
        </button>
      </div>
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      {result && (
        <div role="status" className="space-y-1">
          <p className="font-medium">
            {result.valid ? 'La AEAT no ha encontrado errores.' : 'La AEAT ha encontrado errores:'}
          </p>
          <ul className="list-disc pl-5">
            {result.messages.map((m) => (
              <li key={m}>{m}</li>
            ))}
          </ul>
          {result.pdf_path && <p>Borrador guardado en {result.pdf_path}</p>}
        </div>
      )}
    </div>
  );
}

/** Modalidad y reducciones del IRPF de un ejercicio en el formulario ('' = sin indicar). */
interface IrpfYear {
  regime: IrpfRegime | '';
  reduction: Reduction32;
  over6500: boolean;
  start: boolean;
}
const IRPF_EMPTY: IrpfYear = { regime: '', reduction: 'Ninguna', over6500: false, start: false };

/** Datos de la configuración antigua (sin ejercicio), si los hay. */
function legacyOf(s: ModelSettings): YearSettings | null {
  const legacy: YearSettings = {
    previous_year_net: s.previous_year_net,
    corporate_tax_base: s.corporate_tax_base,
    corporate_tax_base_1p: s.corporate_tax_base_1p ?? null,
    corporate_regime: null,
    corporate_fines: s.corporate_fines,
    corporate_increases: s.corporate_increases,
    corporate_decreases: s.corporate_decreases,
    capitalization_reserve: s.capitalization_reserve,
    pending_losses: s.pending_losses,
  };
  const any =
    legacy.previous_year_net !== null ||
    legacy.corporate_tax_base !== null ||
    legacy.corporate_tax_base_1p !== null ||
    [
      legacy.corporate_fines,
      legacy.corporate_increases,
      legacy.corporate_decreases,
      legacy.capitalization_reserve,
      legacy.pending_losses,
    ].some((n) => n !== 0);
  return any ? legacy : null;
}

/**
 * Datos de un ejercicio: los suyos o, si no tiene, los de la configuración antigua (sin año),
 * que siguen valiendo para los ejercicios sin datos propios.
 */
function yearOf(s: ModelSettings, fy: number): { ys: YearSettings; legacy: boolean } {
  const own = s.years?.[String(fy)];
  if (own) return { ys: own, legacy: false };
  const legacy = legacyOf(s);
  if (legacy) return { ys: legacy, legacy: true };
  const empty: YearSettings = {
    previous_year_net: null,
    corporate_tax_base: null,
    corporate_tax_base_1p: null,
    corporate_regime: null,
    corporate_fines: 0,
    corporate_increases: 0,
    corporate_decreases: 0,
    capitalization_reserve: 0,
    pending_losses: 0,
  };
  return { ys: empty, legacy: false };
}

function SettingsForm({
  onSaved,
  year,
  persona = null,
}: {
  onSaved: () => void;
  year: number;
  /** Sin perfil, se enseña todo; si no, sólo lo del IRPF (personas) o del IS (sociedades). */
  persona?: TaxpayerProfile['persona'] | null;
}) {
  const showIs = persona !== 'Fisica' && persona !== 'EntidadAtribucion';
  const showIrpf = persona !== 'Juridica';
  const [s, setS] = useState<ModelSettings | null>(null);
  const [fy, setFy] = useState(year);
  const [regime, setRegime] = useState<string>('');
  const [carried, setCarried] = useState('');
  const [compYear, setCompYear] = useState('');
  const [prevNet, setPrevNet] = useState('');
  const [isBase, setIsBase] = useState('');
  const [isBase1p, setIsBase1p] = useState('');
  const [corp, setCorp] = useState({ fines: '', inc: '', dec: '', res: '', losses: '' });
  // Autónomos: modalidad de estimación directa y reducciones de la Renta del ejercicio.
  const [irpf, setIrpf] = useState<IrpfYear>(IRPF_EMPTY);
  const [msg, setMsg] = useState<string | null>(null);
  // Los datos del ejercicio en pantalla se han tocado (sólo entonces se guardan para ese año).
  const [yearDirty, setYearDirty] = useState(false);
  const [fromLegacy, setFromLegacy] = useState(false);
  // Datos del ejercicio `y` en el formulario.
  const pickYear = (v: ModelSettings, y: number) => {
    const { ys, legacy } = yearOf(v, y);
    setFromLegacy(legacy);
    setYearDirty(false);
    const e = (n: number) => (n ? euros(n) : '');
    setPrevNet(ys.previous_year_net === null ? '' : euros(ys.previous_year_net));
    setIsBase(ys.corporate_tax_base === null ? '' : euros(ys.corporate_tax_base));
    setIsBase1p(ys.corporate_tax_base_1p === null ? '' : euros(ys.corporate_tax_base_1p));
    setRegime(ys.corporate_regime ?? '');
    setCorp({
      fines: e(ys.corporate_fines),
      inc: e(ys.corporate_increases),
      dec: e(ys.corporate_decreases),
      res: e(ys.capitalization_reserve),
      losses: e(ys.pending_losses),
    });
    setIrpf({
      regime: ys.irpf_regime ?? '',
      reduction: ys.activity_reduction ?? 'Ninguna',
      over6500: ys.other_income_over_6500 ?? false,
      start: ys.activity_start ?? false,
    });
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: se carga una vez (con el año inicial)
  useEffect(() => {
    modelSettings()
      .then((v) => {
        setS(v);
        setCarried(v.vat_compensation_carried ? euros(v.vat_compensation_carried) : '');
        setCompYear(v.vat_compensation_year ? String(v.vat_compensation_year) : '');
        pickYear(v, year);
      })
      .catch((e) => setMsg(errorMessage(e)));
  }, []);

  if (!s) return msg ? <p role="alert">{msg}</p> : null;
  return (
    <form
      className="mt-4 space-y-5 rounded-lg border border-stone-200 bg-white p-4 text-sm"
      onSubmit={async (e) => {
        e.preventDefault();
        const vat = carried.trim() ? parseEuros(carried) : 0;
        const net = prevNet.trim() ? parseEuros(prevNet) : null;
        const base = isBase.trim() ? parseEuros(isBase) : null;
        const base1p = isBase1p.trim() ? parseEuros(isBase1p) : null;
        const amount = (t: string) => (t.trim() ? parseEuros(t) : 0);
        const c = {
          fines: amount(corp.fines),
          inc: amount(corp.inc),
          dec: amount(corp.dec),
          res: amount(corp.res),
          losses: amount(corp.losses),
        };
        if (
          vat === null ||
          (prevNet.trim() && net === null) ||
          (isBase.trim() && base === null) ||
          (isBase1p.trim() && base1p === null) ||
          (compYear.trim() !== '' && !/^\d{4}$/.test(compYear.trim())) ||
          Object.values(c).some((x) => x === null)
        ) {
          setMsg('Revisa los importes.');
          return;
        }
        try {
          const thisYear: YearSettings = {
            previous_year_net: net,
            corporate_tax_base: base,
            corporate_tax_base_1p: base1p,
            corporate_regime: (regime || null) as YearSettings['corporate_regime'],
            corporate_fines: c.fines ?? 0,
            corporate_increases: c.inc ?? 0,
            corporate_decreases: c.dec ?? 0,
            capitalization_reserve: c.res ?? 0,
            pending_losses: c.losses ?? 0,
            irpf_regime: irpf.regime || null,
            activity_reduction: irpf.reduction,
            other_income_over_6500: irpf.over6500,
            activity_start: irpf.start,
          };
          // Los datos del ejercicio sólo se guardan si se han tocado o ya eran suyos: cambiar
          // otro ajuste no convierte los de la configuración antigua en los de este año. Los
          // antiguos (sin año) se quedan para los ejercicios que aún no tienen los suyos.
          const own = Boolean(s.years?.[String(fy)]);
          const next: ModelSettings = {
            ...s,
            vat_compensation_carried: vat,
            vat_compensation_year: compYear.trim() ? Number(compYear.trim()) : null,
            years:
              yearDirty || own ? { ...(s.years ?? {}), [String(fy)]: thisYear } : (s.years ?? {}),
          };
          await saveModelSettings(next);
          setS(next);
          if (yearDirty || own) setFromLegacy(false);
          setYearDirty(false);
          setMsg('Guardado.');
          onSaved();
        } catch (err) {
          setMsg(errorMessage(err));
        }
      }}
    >
      <fieldset className="form-grid">
        <legend className="span-full float-left font-medium">Domiciliación</legend>
        <label className="span-2">
          IBAN para domiciliar y devoluciones
          <input
            className="font-mono"
            value={s.iban ?? ''}
            onChange={(e) => setS({ ...s, iban: e.target.value.replace(/\s/g, '') || null })}
            placeholder="ES00 0000 0000 0000 0000 0000"
          />
        </label>
        <label className="span-2 flex gap-2">
          <input
            type="checkbox"
            role="switch"
            aria-checked={s.domiciliar}
            checked={s.domiciliar}
            onChange={(e) => setS({ ...s, domiciliar: e.target.checked })}
          />
          Domiciliar los pagos
        </label>
      </fieldset>
      <fieldset className="form-grid border-t border-stone-200 pt-4">
        <legend className="span-full float-left font-medium">IVA</legend>
        <label>
          IVA a compensar de años anteriores (303)
          <input
            className="text-right"
            inputMode="decimal"
            value={carried}
            onChange={(e) => setCarried(e.target.value)}
            placeholder="0,00"
          />
        </label>
        <label className="span-2">
          Año en que empieza esa compensación
          <input
            inputMode="numeric"
            value={compYear}
            onChange={(e) => setCompYear(e.target.value)}
            placeholder="El primero sin datos en TaxPain"
          />
        </label>
        <label className="span-2">
          Prorrata del IVA: porcentaje provisional (el definitivo del año pasado; vacío si todo tu
          IVA es deducible)
          <input
            className="max-w-32"
            inputMode="numeric"
            value={s.prorata ?? ''}
            onChange={(e) => {
              const n = Number.parseInt(e.target.value, 10);
              setS({ ...s, prorata: Number.isNaN(n) ? null : Math.max(0, Math.min(100, n)) });
            }}
          />
        </label>
        <div className="span-full flex flex-col gap-2">
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={s.request_refund}
              onChange={(e) => setS({ ...s, request_refund: e.target.checked })}
            />
            En el 4T, pedir la devolución del IVA en vez de compensar
          </label>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={s.exempt_from_390}
              onChange={(e) => setS({ ...s, exempt_from_390: e.target.checked })}
            />
            Exonerado del modelo 390 (el 303 del 4T llevará el resumen del año: actividad y volumen
            de operaciones)
          </label>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={s.cash_basis ?? false}
              onChange={(e) => setS({ ...s, cash_basis: e.target.checked })}
            />
            Criterio de caja del IVA: declaro el IVA al cobrar y lo deduzco al pagar
          </label>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={s.redeme ?? false}
              onChange={(e) => setS({ ...s, redeme: e.target.checked })}
            />
            Inscrita en el registro de devolución mensual (REDEME): 303 de cada mes, devolución cada
            mes y libros por el SII
          </label>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={s.sii_voluntario ?? false}
              onChange={(e) => setS({ ...s, sii_voluntario: e.target.checked })}
            />
            Acogida voluntariamente al SII
          </label>
        </div>
        {s.prorata != null && (
          <div className="span-full">
            <ProrataExtra s={s} setS={setS} />
          </div>
        )}
      </fieldset>
      <fieldset className="form-grid border-t border-stone-200 pt-4" hidden={!showIs}>
        <legend className="span-full float-left font-medium">Impuesto sobre Sociedades</legend>
        <label>
          CNAE de la actividad principal (202)
          <input
            className="font-mono"
            value={s.cnae}
            onChange={(e) => setS({ ...s, cnae: e.target.value.trim() })}
            maxLength={4}
          />
        </label>
        <label>
          Tipo de gravamen del IS (202)
          <input
            value={s.corporate_tax_rate}
            onChange={(e) => setS({ ...s, corporate_tax_rate: e.target.value })}
            placeholder="p. ej. 25"
          />
        </label>
        <label className="span-2">
          Tipo de gravamen del IS (200), por defecto
          <select
            value={s.corporate_regime}
            onChange={(e) =>
              setS({ ...s, corporate_regime: e.target.value as ModelSettings['corporate_regime'] })
            }
          >
            {REGIMES.map((k) => (
              <option key={k} value={k}>
                {regimeLabels()[k]}
              </option>
            ))}
          </select>
        </label>
        <label>
          Modelo de cuentas anuales (200)
          <select
            value={s.accounts_model ?? 'Pymes'}
            onChange={(e) =>
              setS({ ...s, accounts_model: e.target.value as 'Pymes' | 'Abreviado' | 'Normal' })
            }
          >
            <option value="Pymes">PYMES</option>
            <option value="Abreviado">Abreviado</option>
            <option value="Normal">Normal</option>
          </select>
        </label>
        <div className="span-full flex flex-col gap-2">
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={s.turnover_under_1m}
              onChange={(e) => setS({ ...s, turnover_under_1m: e.target.checked })}
            />
            Cifra de negocios del año anterior inferior a 1 millón
          </label>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={s.turnover_over_6m}
              onChange={(e) => setS({ ...s, turnover_over_6m: e.target.checked })}
            />
            Cifra de negocios superior a 6 millones
          </label>
        </div>
      </fieldset>
      <fieldset className="form-grid rounded-md border border-stone-200 p-4 pt-3">
        <legend className="px-1 font-medium">
          Datos de cada ejercicio (
          {showIs && showIrpf ? '130, Renta, 202 y 200' : showIrpf ? '130 y Renta' : '202 y 200'})
        </legend>
        <div className="span-full flex flex-wrap items-center gap-3">
          <YearStepper
            year={fy}
            onChange={(y) => {
              if (yearDirty) {
                setMsg(`Guarda o descarta antes los cambios de ${fy}.`);
                return;
              }
              setMsg(null);
              setFy(y);
              pickYear(s, y);
            }}
            label="Ejercicio de estos datos"
          />
          {yearDirty && (
            <button type="button" className="text-link" onClick={() => pickYear(s, fy)}>
              Descartar los cambios de {fy}
            </button>
          )}
        </div>
        {fromLegacy && (
          <div className="span-full rounded-md bg-amber-50 p-3 text-amber-900">
            <p>
              Estos datos son de la configuración antigua, sin ejercicio: se usan (con aviso) en los
              años que aún no tienen los suyos. Si son de {fy}, revísalos y guárdalos.
            </p>
            <button
              type="button"
              className="text-link mt-1"
              onClick={async () => {
                const next: ModelSettings = {
                  ...s,
                  previous_year_net: null,
                  corporate_tax_base: null,
                  corporate_tax_base_1p: null,
                  corporate_fines: 0,
                  corporate_increases: 0,
                  corporate_decreases: 0,
                  capitalization_reserve: 0,
                  pending_losses: 0,
                };
                try {
                  await saveModelSettings(next);
                  setS(next);
                  pickYear(next, fy);
                  setMsg('Configuración antigua borrada.');
                  onSaved();
                } catch (err) {
                  setMsg(errorMessage(err));
                }
              }}
            >
              Ya tengo los datos de cada año: borrar la configuración antigua
            </button>
          </div>
        )}
        <label className="span-2" hidden={!showIrpf}>
          Estimación directa del IRPF en {fy} (autónomos: 130 y Renta)
          <select
            value={irpf.regime}
            onChange={(e) => {
              setIrpf({ ...irpf, regime: e.target.value as IrpfYear['regime'] });
              setYearDirty(true);
            }}
          >
            <option value="">Sin indicar (simplificada)</option>
            <option value="Simplificada">Simplificada (lo habitual)</option>
            <option value="Normal">
              Normal (más de 600.000 € de cifra de negocios o renuncia)
            </option>
          </select>
          <span className="mt-1 block text-xs text-stone-500">
            En simplificada se deduce además un 5 % del rendimiento neto (como máximo 2.000 € al
            año) por provisiones y gastos de difícil justificación.
          </span>
        </label>
        <label className="span-2" hidden={!showIrpf}>
          Reducción por la actividad en la Renta de {fy} (art. 32.2)
          <select
            value={irpf.reduction}
            onChange={(e) => {
              setIrpf({ ...irpf, reduction: e.target.value as Reduction32 });
              setYearDirty(true);
            }}
          >
            <option value="Ninguna">Ninguna</option>
            <option value="Dependiente">Autónomo dependiente o con un único cliente</option>
            <option value="RentasBajas">Rendimiento neto de menos de 12.000 €</option>
          </select>
        </label>
        <div className="span-full flex flex-col gap-2" hidden={!showIrpf}>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={irpf.over6500}
              onChange={(e) => {
                setIrpf({ ...irpf, over6500: e.target.checked });
                setYearDirty(true);
              }}
            />
            En {fy} tuve otras rentas (sueldos, alquileres, intereses…) de más de 6.500 €
          </label>
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              checked={irpf.start}
              onChange={(e) => {
                setIrpf({ ...irpf, start: e.target.checked });
                setYearDirty(true);
              }}
            />
            Reducción del 20 % por inicio de actividad (primer año con beneficio y el siguiente)
          </label>
        </div>
        <label className="span-2" hidden={!showIrpf}>
          Rendimiento neto de {fy - 1} (130 de {fy})
          <input
            value={prevNet}
            onChange={(e) => {
              setPrevNet(e.target.value);
              setYearDirty(true);
            }}
            placeholder={`Vacío: se calcula con la contabilidad de ${fy - 1}`}
          />
        </label>
        <label className="span-2" hidden={!showIs}>
          Cuota del IS de {fy - 1} (202 de {fy}, 2P y 3P)
          <input
            value={isBase}
            onChange={(e) => {
              setIsBase(e.target.value);
              setYearDirty(true);
            }}
            placeholder={`Vacío: la [00599] del 200 de ${fy - 1} presentado`}
          />
        </label>
        <label className="span-2" hidden={!showIs}>
          Cuota del IS de {fy - 2} (202 de {fy}, 1P de abril)
          <input
            value={isBase1p}
            onChange={(e) => {
              setIsBase1p(e.target.value);
              setYearDirty(true);
            }}
            placeholder={`Vacío: la [00599] del 200 de ${fy - 2} presentado`}
          />
        </label>
        <label hidden={!showIs}>
          Tipo de gravamen del IS de {fy} (200)
          <select
            value={regime}
            onChange={(e) => {
              setRegime(e.target.value);
              setYearDirty(true);
            }}
          >
            <option value="">El de por defecto</option>
            {REGIMES.map((k) => (
              <option key={k} value={k}>
                {regimeLabels(fy)[k]}
              </option>
            ))}
          </select>
        </label>
        {(
          [
            ['fines', 'Multas y sanciones no deducibles (200)'],
            ['inc', 'Otras correcciones al resultado en aumento (200)'],
            ['dec', 'Correcciones al resultado en disminución (200)'],
            ['res', 'Reserva de capitalización (200)'],
            ['losses', 'Bases negativas pendientes de compensar (200)'],
          ] as const
        ).map(([k, label]) => (
          <label key={k} hidden={!showIs}>
            {label}
            <input
              className="text-right"
              value={corp[k]}
              onChange={(e) => {
                setCorp({ ...corp, [k]: e.target.value });
                setYearDirty(true);
              }}
              placeholder="0,00"
            />
          </label>
        ))}
        <p className="span-full text-xs text-stone-500">
          Cada ejercicio guarda los suyos: los de un año no se usan en el siguiente.
          {showIs &&
            ' La cuota del IS también puede salir del 200 del gestor tecleado en Importar → Comparar con la gestoría.'}
        </p>
      </fieldset>
      <div className="form-actions">
        <button type="submit" className="rounded btn-primary px-3 py-1.5">
          Guardar
        </button>
        {msg && <span role="status">{msg}</span>}
      </div>
    </form>
  );
}

export function Models({
  rectificar,
}: {
  /** Abrir con la rectificativa de ese periodo preparada (desde Recibidas). */
  rectificar?: Rectificable | undefined;
} = {}) {
  const initial = rectificar
    ? { year: rectificar.ejercicio, quarter: rectificar.trimestre || 1 }
    : currentQuarter();
  const [model, setModel] = useState(rectificar?.modelo ?? '303');
  const [year, setYear] = useState(initial.year);
  const [quarter, setQuarter] = useState(initial.quarter);
  /** Lo presentado del periodo (si lo está): ofrece «Preparar rectificativa…». */
  const [anterior, setAnterior] = useState<RectificativaAnterior | null>(null);
  const [rectifying, setRectifying] = useState(rectificar != null);
  const [draft, setDraft] = useState<ModelDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [prevalidating, setPrevalidating] = useState(false);
  const [filing, setFiling] = useState(false);
  const [filedVersion, setFiledVersion] = useState(0);
  const ia = useAskAi();
  // El cotejo con los datos fiscales de Renta es de personas físicas: una sociedad no lo tiene
  // (el proceso nativo rechaza el certificado de representante).
  const [persona, setPersona] = useState<TaxpayerProfile['persona'] | null>(null);
  useEffect(() => {
    getProfile()
      .then((p) => setPersona(p?.persona ?? null))
      .catch(() => setPersona(null));
  }, []);
  const prevalidateBtn = useRef<HTMLButtonElement>(null);
  const fileBtn = useRef<HTMLButtonElement>(null);
  useReturnFocus(prevalidating, prevalidateBtn);
  useReturnFocus(filing, fileBtn);
  const current = MODELS.find((m) => m.id === model);
  const annual = current?.annual ?? false;
  const periods = current?.periods ?? ['1T', '2T', '3T', '4T'];
  /** El perfil dice que este modelo se presenta cada mes (gran empresa, REDEME, 349 mensual). */
  const [monthly, setMonthly] = useState(false);
  useEffect(() => {
    setMonthly(false);
    if (!MENSUALES.includes(model)) return;
    let live = true;
    modelMonthly(model)
      .then((m) => {
        if (live) setMonthly(m === true);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [model]);
  const withMonths = MENSUALES.includes(model) && (monthly || quarter > MES);
  // El 202 tiene tres periodos: en ene–mar el trimestre «en curso» (4T) no existe para él. Un
  // mes (101–112) sólo vale en los modelos que pueden ser mensuales.
  const q = annual
    ? 0
    : quarter > MES
      ? MENSUALES.includes(model)
        ? quarter
        : Math.min(Math.ceil((quarter - MES) / 3), periods.length)
      : Math.min(quarter, periods.length);

  const load = useCallback(() => {
    setError(null);
    setMsg(null);
    if (EXTRANJERO.includes(model)) {
      setDraft(null);
      return;
    }
    modelDraft(model, year, q)
      .then(setDraft)
      .catch((e) => {
        setDraft(null);
        setError(errorMessage(e));
      });
  }, [model, year, q]);
  useEffect(load, [load]);
  // Al cambiar de modelo o de periodo se cierra la rectificativa (salvo al abrir con ella).
  const period = `${model}-${year}-${q}`;
  const opened = useRef(period);
  useEffect(() => {
    if (opened.current !== period) {
      opened.current = period;
      setRectifying(false);
    }
  }, [period]);
  useEffect(() => {
    setAnterior(null);
    if (!RECTIFICABLES.includes(model)) return;
    let live = true;
    rectificativaAnterior(model, year, q)
      .then((a) => {
        if (live) setAnterior(a);
      })
      .catch(() => {
        if (live) setAnterior(null);
      });
    return () => {
      live = false;
    };
  }, [model, year, q]);

  return (
    <section aria-labelledby="modelos-h" className="mt-6">
      <h2 id="modelos-h" className="text-lg font-semibold">
        Modelos
      </h2>
      <p className="mt-1 text-sm text-stone-600">
        Borrador calculado con tus facturas y tu contabilidad. Puedes guardar el fichero para la
        Sede de la AEAT («Presentación mediante fichero»), prevalidarlo con la AEAT (envía los
        datos, pero no presenta) o presentarlo en el entorno de pruebas. La presentación real
        llegará con la versión firmada: hasta entonces, revisa y presenta tú.
      </p>
      <div className="mt-4 flex flex-wrap items-end gap-3 text-sm">
        <label>
          Modelo
          <select className="mt-1 block" value={model} onChange={(e) => setModel(e.target.value)}>
            {MODELS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
        </label>
        <div>
          <span aria-hidden="true" className="block text-[0.8125rem] font-medium text-stone-700">
            Ejercicio
          </span>
          <div className="mt-[0.3rem]">
            <YearStepper year={year} onChange={setYear} />
          </div>
        </div>
        {!annual && (
          <label>
            {withMonths ? 'Periodo' : 'Trimestre'}
            <select
              className="mt-1 block"
              value={q}
              onChange={(e) => setQuarter(Number(e.target.value))}
            >
              {withMonths ? (
                <>
                  <optgroup label="Meses">
                    {MESES.map((m, i) => (
                      <option key={m} value={MES + i + 1}>
                        {m}
                      </option>
                    ))}
                  </optgroup>
                  <optgroup label="Trimestres (comprobación)">
                    {periods.map((p, i) => (
                      <option key={p} value={i + 1}>
                        {p}
                      </option>
                    ))}
                  </optgroup>
                </>
              ) : (
                periods.map((p, i) => (
                  <option key={p} value={i + 1}>
                    {p}
                  </option>
                ))
              )}
            </select>
          </label>
        )}
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={() => setShowSettings(!showSettings)}
          aria-expanded={showSettings}
        >
          Datos de los modelos
        </button>
        {draft?.file && (
          <button
            type="button"
            className="rounded btn-primary px-3 py-1.5"
            onClick={async () => {
              const dir = await pickFolder();
              if (!dir) return;
              try {
                setMsg(`Fichero guardado en ${await modelExport(model, year, q, dir)}`);
              } catch (e) {
                setMsg(errorMessage(e));
              }
            }}
          >
            Guardar fichero para la Sede…
          </button>
        )}
      </div>
      <div className="form-actions mt-3 text-sm empty:hidden">
        {draft?.file && PREVALIDATABLE.includes(model) && !prevalidating && (
          <button
            ref={prevalidateBtn}
            type="button"
            className="rounded border border-amber-400 px-3 py-1"
            onClick={() => setPrevalidating(true)}
          >
            Prevalidar con la AEAT…
          </button>
        )}
        {draft?.file && PRESENTABLE.includes(model) && !filing && (
          <button
            ref={fileBtn}
            type="button"
            className="rounded border border-red-300 px-3 py-1"
            onClick={() => setFiling(true)}
          >
            Presentar en el entorno de pruebas…
          </button>
        )}
      </div>
      {anterior && (
        <div className="mt-3 flex flex-wrap items-center gap-3 text-sm">
          <p className="text-stone-700">{anteriorTexto(anterior)}</p>
          {!rectifying && (
            <button
              type="button"
              className="rounded border border-amber-400 px-3 py-1"
              onClick={() => setRectifying(true)}
            >
              Preparar rectificativa…
            </button>
          )}
        </div>
      )}
      {anterior && rectifying && (
        <RectificativaPanel
          key={`rect-${model}-${year}-${q}`}
          model={model}
          year={year}
          quarter={q}
          anterior={anterior}
          labels={BOX_LABELS[model] ?? {}}
          onClose={() => setRectifying(false)}
        />
      )}
      {filing && draft && (
        <FileTesting
          key={`${model}-${year}-${q}`}
          draft={draft}
          onClose={() => setFiling(false)}
          onFiled={() => setFiledVersion((n) => n + 1)}
        />
      )}
      {prevalidating && (
        <Prevalidate
          key={`${model}-${year}-${q}`}
          model={model}
          year={year}
          quarter={q}
          onClose={() => setPrevalidating(false)}
        />
      )}
      {draft && model === '347' && <Letters347 year={year} />}
      {(model === '180' || model === '115') && (
        <InmueblesArrendados
          key={`inm-${model}-${year}`}
          arrendadores={model === '180' && draft ? arrendadoresDe(draft.details) : []}
          onChange={load}
        />
      )}
      {(model === '216' || model === '296') && (
        <NoResidentes key={`irnr-${year}`} year={year} onChange={load} />
      )}
      {draft && ['190', '180', '193', '296'].includes(model) && (
        <Certificados key={`cert-${model}-${year}`} model={model} year={year} />
      )}
      {model === '184' && <Entity184 onChange={load} />}
      {model === '202' && <Pago202 key={`pago202-${year}`} year={year} onChange={load} />}
      {model === '200' && <Modelo200 key={`m200-${year}`} year={year} onChange={load} />}
      {(model === '720' || model === '721') && (
        <BienesExtranjero key={`ext-${model}-${year}`} model={model} year={year} />
      )}
      {draft && (
        <ModelAdjust
          key={`ajuste-${model}-${year}-${q}`}
          model={model}
          year={year}
          quarter={q}
          onChange={load}
        />
      )}
      {/* El pago de un mes (y su asiento de liquidación del IVA) aún va por trimestres. */}
      {draft && q <= MES && (
        <TaxPaymentPanel
          key={`pago-${model}-${year}-${q}-${filedVersion}`}
          model={model}
          year={year}
          quarter={q}
          amount={draft.result}
        />
      )}
      {showSettings && <SettingsForm onSaved={load} year={year} persona={persona} />}
      {msg && (
        <p role="status" className="mt-3 text-sm">
          {msg}
        </p>
      )}
      {error && (
        <p role="alert" className="mt-4 text-sm text-red-700">
          {error}
        </p>
      )}
      {draft && (
        <DraftView
          d={draft}
          questions={ia.questions(draftTopic(draft))}
          onAsk={(b) =>
            ia.open(
              {
                topic: draftTopic(draft),
                title: draftTitle(draft.model, draft.year, draft.quarter),
                conversationId: null,
              },
              b,
            )
          }
        />
      )}
      <FiledList year={year} version={filedVersion} />
      {persona === 'Fisica' && <RentaActivity year={new Date().getFullYear() - 1} />}
      {persona === 'Fisica' && <RentaCheck year={new Date().getFullYear() - 1} />}
      <AeatQuery key={`${model}-${year}`} model={model} year={year} />
      {model === '303' && <SiiPanel />}
      <PresentadoAeat year={year} />
      {ia.element}
    </section>
  );
}

/** Prorrata especial, prorratas de años anteriores (para regularizar bienes de inversión) y
 * actividad para el 390. */
function ProrataExtra({ s, setS }: { s: ModelSettings; setS: (s: ModelSettings) => void }) {
  const history = s.prorata_history ?? {};
  const [year, setYear] = useState('');
  const [pct, setPct] = useState('');
  const years = Object.keys(history).sort();
  return (
    <fieldset className="flex flex-col gap-3 rounded-md border border-stone-200 p-4 pt-3">
      <legend className="px-1 font-medium">Prorrata</legend>
      <label className="flex items-start gap-2">
        <input
          type="checkbox"
          checked={s.prorata_special ?? false}
          onChange={(e) => setS({ ...s, prorata_special: e.target.checked })}
        />
        Prorrata especial: al revisar cada compra indicas si es de uso común, sólo para lo gravado o
        sólo para lo exento
      </label>
      <label className="block max-w-md">
        Actividad (apartado de prorratas del 390)
        <input
          className="mt-1 block w-full"
          maxLength={40}
          value={s.activity_description ?? ''}
          onChange={(e) => setS({ ...s, activity_description: e.target.value })}
        />
      </label>
      <p className="text-sm text-stone-600">
        Prorratas definitivas de años anteriores a TaxPain: sirven para regularizar el IVA de los
        bienes de inversión comprados esos años (casilla [43] del último trimestre).
      </p>
      {years.length > 0 && (
        <ul className="text-sm">
          {years.map((y) => (
            <li key={y} className="flex items-center gap-2">
              {y}: {history[y]} %
              <button
                type="button"
                className="text-red-700 underline"
                onClick={() => {
                  const { [y]: _, ...rest } = history;
                  setS({ ...s, prorata_history: rest });
                }}
              >
                Quitar {y}
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex flex-wrap items-end gap-3">
        <label>
          Año
          <input
            className="mt-1 block w-24"
            inputMode="numeric"
            value={year}
            onChange={(e) => setYear(e.target.value)}
          />
        </label>
        <label>
          % definitivo
          <input
            className="mt-1 block w-24"
            inputMode="numeric"
            value={pct}
            onChange={(e) => setPct(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-stone-300 px-3 py-1"
          onClick={() => {
            const y = Number.parseInt(year, 10);
            const p = Number.parseInt(pct, 10);
            if (Number.isNaN(y) || Number.isNaN(p) || y < 2000 || p < 0 || p > 100) return;
            setS({ ...s, prorata_history: { ...history, [String(y)]: p } });
            setYear('');
            setPct('');
          }}
        >
          Añadir prorrata
        </button>
      </div>
    </fieldset>
  );
}
