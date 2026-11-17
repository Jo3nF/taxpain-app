import { useCallback, useEffect, useState } from 'react';
import {
  type CertificadoAdmin,
  type CertificadoAdminVista,
  type CompanyDoc,
  certificadoAdminDelete,
  certificadoAdminDesdeDocumento,
  certificadoAdminSave,
  certificadosAdminList,
  companyDocsList,
  type EscritoOpciones,
  type EscritoTipo,
  type Expediente,
  type ExpedienteActuacion,
  type ExpedienteCalculo,
  type ExpedienteOrganismo,
  type ExpedienteTipo,
  type ExpedienteVista,
  errorMessage,
  euros,
  expedienteActuacionAdd,
  expedienteActuacionDelete,
  expedienteCreate,
  expedienteDelete,
  expedienteDesdeDocumento,
  expedienteEscritoSave,
  expedientePreview,
  expedientesList,
  expedienteUpdate,
  formatDate,
  parseEuros,
  type TipoActuacion,
  type TipoCertificadoAdmin,
} from './api';
import { ConfirmAction } from './ConfirmAction';

export const ORGANISMOS: Record<ExpedienteOrganismo, string> = {
  aeat: 'Agencia Tributaria (AEAT)',
  dehu: 'DEHú (Dirección Electrónica Habilitada única)',
  seguridad_social: 'Seguridad Social',
  autonomica: 'Agencia tributaria autonómica',
  ayuntamiento: 'Ayuntamiento',
  otro: 'Otro organismo',
};

export const TIPOS: Record<ExpedienteTipo, string> = {
  requerimiento: 'Requerimiento de información',
  propuesta_liquidacion: 'Propuesta de liquidación',
  audiencia: 'Trámite de audiencia',
  propuesta_sancion: 'Propuesta de sanción',
  liquidacion: 'Liquidación',
  acuerdo_sancionador: 'Acuerdo sancionador',
  providencia_apremio: 'Providencia de apremio',
  diligencia_embargo: 'Diligencia de embargo',
  otro: 'Otra notificación',
};

const ESTADOS: Record<Expediente['estado'], string> = {
  abierto: 'Abierto',
  contestado: 'Contestado',
  recurrido: 'Recurrido',
  cerrado: 'Cerrado',
};

const ACTUACIONES: Record<TipoActuacion, string> = {
  contestacion: 'Contestación',
  alegaciones: 'Alegaciones',
  recurso_reposicion: 'Recurso de reposición',
  reclamacion_tea: 'Reclamación económico-administrativa',
  aplazamiento: 'Solicitud de aplazamiento',
  ampliacion: 'Solicitud de ampliación del plazo',
  pago: 'Pago',
  otro: 'Otra',
};

const ESCRITOS: Record<EscritoTipo, string> = {
  contestacion: 'Contestación al requerimiento',
  alegaciones: 'Alegaciones',
  recurso_reposicion: 'Recurso de reposición',
  aplazamiento: 'Aplazamiento o fraccionamiento',
  ampliacion: 'Ampliación del plazo',
};

/** Escritos que tienen sentido para cada tipo (igual que `escritos::disponibles`). */
export function escritosDe(tipo: ExpedienteTipo): EscritoTipo[] {
  switch (tipo) {
    case 'requerimiento':
      return ['contestacion', 'ampliacion'];
    case 'propuesta_liquidacion':
    case 'audiencia':
      return ['alegaciones', 'ampliacion'];
    case 'propuesta_sancion':
      return ['alegaciones'];
    case 'liquidacion':
    case 'acuerdo_sancionador':
    case 'providencia_apremio':
      return ['recurso_reposicion', 'aplazamiento'];
    case 'diligencia_embargo':
      return ['recurso_reposicion'];
    default:
      return ['contestacion', 'alegaciones'];
  }
}

const CERT_TIPOS: Record<TipoCertificadoAdmin, string> = {
  corriente_aeat: 'Estar al corriente con la AEAT',
  corriente_ss: 'Estar al corriente con la Seguridad Social',
  residencia_fiscal: 'Residencia fiscal',
  situacion_censal: 'Situación censal',
  apoderamiento: 'Apoderamiento',
  otro: 'Otro certificado',
};

export function vacio(): Expediente {
  return {
    id: 0,
    organismo: 'aeat',
    organo: '',
    referencia: '',
    tipo: 'requerimiento',
    asunto: '',
    concepto: '',
    puesto_el: null,
    notificado_el: null,
    plazo_dias: null,
    vence_manual: null,
    importe: null,
    sancion: null,
    fin_voluntario: null,
    estado: 'abierto',
    notice_id: null,
    document_id: null,
    documentos: [],
    notas: '',
  };
}

function pillDias(dias: number): string {
  if (dias <= 2) return 'pill-red font-semibold';
  if (dias <= 7) return 'pill-amber';
  return 'pill-stone';
}

function quedan(dias: number): string {
  if (dias === 0) return 'Último día: hoy';
  if (dias === 1) return 'Último día: mañana';
  return `Quedan ${dias} días`;
}

function titulo(e: Expediente): string {
  return e.referencia ? `${TIPOS[e.tipo]} ${e.referencia}` : `${TIPOS[e.tipo]} · ${e.asunto}`;
}

/** Plazos, recargos y reducciones de un expediente. */
function CalculoView({ c }: { c: ExpedienteCalculo }) {
  return (
    <div className="mt-2 space-y-2 text-sm">
      {c.plazos.length > 0 && (
        <table className="w-full text-left text-xs">
          <caption className="sr-only">Plazos</caption>
          <thead>
            <tr className="text-stone-500">
              <th scope="col" className="py-1 font-medium">
                Plazo
              </th>
              <th scope="col" className="py-1 font-medium">
                Último día
              </th>
              <th scope="col" className="py-1 font-medium">
                Cómo se cuenta
              </th>
            </tr>
          </thead>
          <tbody>
            {c.plazos.map((p) => (
              <tr key={p.clave} className="border-t border-stone-200 align-top">
                <td className="py-1 pr-2">{p.titulo}</td>
                <td className="py-1 pr-2 font-medium tabular-nums">{formatDate(p.vence)}</td>
                <td className="py-1 text-stone-600">
                  {p.computo}. <span className="text-stone-500">{p.norma}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {c.recargo.length > 0 && (
        <div>
          <p className="font-medium">Lo que costaría pagar (art. 28 LGT)</p>
          <ul className="mt-1 space-y-1">
            {c.recargo.map((e) => (
              <li key={e.texto} className={e.hoy ? 'font-medium' : 'text-stone-600'}>
                {e.hoy && <span className="pill pill-amber mr-2">Hoy</span>}
                {e.texto}: {euros(e.total)}
                {e.recargo > 0 && ` (recargo ${euros(e.recargo)}`}
                {e.intereses > 0 && `, intereses ${euros(e.intereses)}`}
                {e.recargo > 0 && ')'}
              </li>
            ))}
          </ul>
        </div>
      )}
      {c.reducciones && (
        <div>
          <p className="font-medium">Sanción con reducciones (art. 188 LGT)</p>
          <dl className="mt-1 grid grid-cols-[auto_auto] justify-start gap-x-4 tabular-nums">
            <dt>Sin reducción</dt>
            <dd>{euros(c.reducciones.sancion)}</dd>
            <dt>Con conformidad (−30 %)</dt>
            <dd>{euros(c.reducciones.conformidad)}</dd>
            <dt>Con pronto pago (−40 %)</dt>
            <dd>{euros(c.reducciones.pronto_pago)}</dd>
            <dt>Con las dos</dt>
            <dd className="font-medium">{euros(c.reducciones.ambas)}</dd>
          </dl>
          <ul className="mt-1 list-disc pl-5 text-xs text-stone-600">
            {c.reducciones.notas.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        </div>
      )}
      {c.avisos.length > 0 && (
        <ul className="list-disc pl-5 text-xs text-stone-600">
          {c.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

function fechaInput(
  label: string,
  value: string | null,
  set: (v: string | null) => void,
  hint?: string,
) {
  return (
    <label className="block text-sm">
      {label}
      <input
        type="date"
        className="input mt-1 w-full"
        value={value ?? ''}
        onChange={(e) => set(e.target.value || null)}
      />
      {hint && <span className="text-xs text-stone-500">{hint}</span>}
    </label>
  );
}

function ImporteInput({
  label,
  value,
  set,
}: {
  label: string;
  value: number | null;
  set: (v: number | null) => void;
}) {
  const [text, setText] = useState(value === null ? '' : euros(value).replace(' €', ''));
  return (
    <label className="block text-sm">
      {label}
      <input
        className="input mt-1 w-full"
        inputMode="decimal"
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          set(e.target.value.trim() ? parseEuros(e.target.value) : null);
        }}
      />
    </label>
  );
}

/** Alta o edición de un expediente, con los plazos que salen mientras se teclea. */
export function ExpedienteForm({
  initial,
  onSaved,
  onCancel,
}: {
  initial: Expediente;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [e, setE] = useState<Expediente>(initial);
  const [calc, setCalc] = useState<ExpedienteCalculo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = <K extends keyof Expediente>(k: K, v: Expediente[K]) =>
    setE((x) => ({ ...x, [k]: v }));
  useEffect(() => {
    if (!e.notificado_el && !e.puesto_el) {
      setCalc(null);
      return;
    }
    expedientePreview(e)
      .then(setCalc)
      .catch(() => setCalc(null));
  }, [e]);
  const conImporte = ['liquidacion', 'acuerdo_sancionador', 'providencia_apremio'].includes(e.tipo);
  const conSancion = ['propuesta_sancion', 'acuerdo_sancionador'].includes(e.tipo);
  return (
    <form
      aria-label={e.id ? 'Editar el expediente' : 'Nuevo expediente'}
      className="card mt-3 space-y-3 p-4"
      onSubmit={async (ev) => {
        ev.preventDefault();
        setBusy(true);
        setError(null);
        try {
          if (e.id) await expedienteUpdate(e.id, e);
          else await expedienteCreate(e);
          onSaved();
        } catch (err) {
          setError(errorMessage(err));
        } finally {
          setBusy(false);
        }
      }}
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="block text-sm">
          Organismo o buzón
          <select
            className="input mt-1 w-full"
            value={e.organismo}
            onChange={(x) => set('organismo', x.target.value as ExpedienteOrganismo)}
          >
            {Object.entries(ORGANISMOS).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          Qué te han notificado
          <select
            className="input mt-1 w-full"
            value={e.tipo}
            onChange={(x) => set('tipo', x.target.value as ExpedienteTipo)}
          >
            {Object.entries(TIPOS).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </label>
        <label className="block text-sm">
          Referencia o número de expediente
          <input
            className="input mt-1 w-full"
            value={e.referencia}
            onChange={(x) => set('referencia', x.target.value)}
          />
        </label>
        <label className="block text-sm">
          Órgano que lo envía
          <input
            className="input mt-1 w-full"
            value={e.organo}
            placeholder="Gestión Tributaria, Dependencia de Inspección…"
            onChange={(x) => set('organo', x.target.value)}
          />
        </label>
        <label className="block text-sm">
          Asunto
          <input
            className="input mt-1 w-full"
            value={e.asunto}
            onChange={(x) => set('asunto', x.target.value)}
          />
        </label>
        <label className="block text-sm">
          Impuesto y ejercicio
          <input
            className="input mt-1 w-full"
            value={e.concepto}
            placeholder="IVA 2025, modelo 303 2T…"
            onChange={(x) => set('concepto', x.target.value)}
          />
        </label>
        {fechaInput(
          'Notificada el',
          e.notificado_el,
          (v) => set('notificado_el', v),
          'El día en que la abriste o la recibiste',
        )}
        {fechaInput(
          'Puesta a disposición el',
          e.puesto_el,
          (v) => set('puesto_el', v),
          'Si aún no la has abierto: se da por notificada a los 10 días naturales',
        )}
        <label className="block text-sm">
          Plazo que da el documento (días hábiles)
          <input
            className="input mt-1 w-full"
            type="number"
            min={1}
            max={365}
            value={e.plazo_dias ?? ''}
            onChange={(x) => set('plazo_dias', x.target.value ? Number(x.target.value) : null)}
          />
        </label>
        {fechaInput(
          'Último día fijado (opcional)',
          e.vence_manual,
          (v) => set('vence_manual', v),
          'Un nuevo plazo concedido o la fecha que diga el escrito',
        )}
        {conImporte && (
          <ImporteInput
            label="Importe de la deuda (€)"
            value={e.importe}
            set={(v) => set('importe', v)}
          />
        )}
        {conSancion && (
          <ImporteInput
            label="Sanción sin reducciones (€)"
            value={e.sancion}
            set={(v) => set('sancion', v)}
          />
        )}
        {e.tipo === 'providencia_apremio' &&
          fechaInput(
            'Fin del periodo voluntario de la deuda',
            e.fin_voluntario,
            (v) => set('fin_voluntario', v),
            'Lo dice la providencia: sirve para los intereses de demora',
          )}
        {e.id !== 0 && (
          <label className="block text-sm">
            Estado
            <select
              className="input mt-1 w-full"
              value={e.estado}
              onChange={(x) => set('estado', x.target.value as Expediente['estado'])}
            >
              {Object.entries(ESTADOS).map(([k, v]) => (
                <option key={k} value={k}>
                  {v}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      <label className="block text-sm">
        Documentos que se aportan (uno por línea; empieza con «✓ » los que ya tienes)
        <textarea
          className="input mt-1 w-full"
          rows={3}
          value={e.documentos.map((d) => `${d.listo ? '✓ ' : ''}${d.descripcion}`).join('\n')}
          onChange={(x) =>
            set(
              'documentos',
              x.target.value.split('\n').map((l) => ({
                descripcion: l.replace(/^✓\s*/, ''),
                listo: l.startsWith('✓'),
              })),
            )
          }
        />
      </label>
      <label className="block text-sm">
        Notas
        <textarea
          className="input mt-1 w-full"
          rows={2}
          value={e.notas}
          onChange={(x) => set('notas', x.target.value)}
        />
      </label>
      {calc && <CalculoView c={calc} />}
      {error && (
        <p role="alert" className="text-sm text-red-700">
          {error}
        </p>
      )}
      <div className="form-actions">
        <button type="submit" className="btn-primary" disabled={busy}>
          {e.id ? 'Guardar los cambios' : 'Crear el expediente'}
        </button>
        <button type="button" className="btn-ghost" onClick={onCancel} disabled={busy}>
          Cancelar
        </button>
      </div>
    </form>
  );
}

function NuevaActuacion({ id, onDone }: { id: number; onDone: (msg?: string) => void }) {
  const [a, setA] = useState<ExpedienteActuacion>({
    id: 0,
    fecha: '',
    tipo: 'contestacion',
    registro: '',
    descripcion: '',
  });
  return (
    <form
      aria-label="Anotar una actuación"
      className="mt-2 flex flex-wrap items-end gap-2 text-sm"
      onSubmit={async (ev) => {
        ev.preventDefault();
        try {
          await expedienteActuacionAdd(id, a);
          onDone('Actuación anotada.');
        } catch (e) {
          onDone(errorMessage(e));
        }
      }}
    >
      <label>
        Fecha
        <input
          type="date"
          required
          className="input ml-1"
          value={a.fecha}
          onChange={(e) => setA({ ...a, fecha: e.target.value })}
        />
      </label>
      <label>
        Qué
        <select
          className="input ml-1"
          value={a.tipo}
          onChange={(e) => setA({ ...a, tipo: e.target.value as TipoActuacion })}
        >
          {Object.entries(ACTUACIONES).map(([k, v]) => (
            <option key={k} value={k}>
              {v}
            </option>
          ))}
        </select>
      </label>
      <label>
        N.º de registro
        <input
          className="input ml-1 w-40"
          value={a.registro}
          onChange={(e) => setA({ ...a, registro: e.target.value })}
        />
      </label>
      <button type="submit" className="btn-ghost">
        Anotar
      </button>
    </form>
  );
}

function Escritos({ x, onMsg }: { x: ExpedienteVista; onMsg: (m: string) => void }) {
  const [tipo, setTipo] = useState<EscritoTipo>(escritosDe(x.expediente.tipo)[0] ?? 'alegaciones');
  const [op, setOp] = useState<EscritoOpciones>({ motivo: '', plazos: null, lugar: '' });
  const save = async (pdf: boolean) => {
    try {
      const p = await expedienteEscritoSave(x.expediente.id, tipo, op, pdf);
      if (p) onMsg(`Borrador guardado en ${p}. Revísalo antes de presentarlo: nada se ha enviado.`);
    } catch (e) {
      onMsg(errorMessage(e));
    }
  };
  return (
    <div className="mt-3 rounded-md border border-stone-200 p-3 text-sm">
      <p className="font-medium">Preparar un escrito</p>
      <p className="text-xs text-stone-600">
        Borrador con los datos del expediente y de la empresa; lo que tienes que escribir va entre
        corchetes. Es un modelo general: en una inspección o una sanción, consulta a un asesor.
      </p>
      <div className="mt-2 flex flex-wrap items-end gap-2">
        <label>
          Escrito
          <select
            className="input ml-1"
            value={tipo}
            onChange={(e) => setTipo(e.target.value as EscritoTipo)}
          >
            {escritosDe(x.expediente.tipo).map((t) => (
              <option key={t} value={t}>
                {ESCRITOS[t]}
              </option>
            ))}
          </select>
        </label>
        {(tipo === 'aplazamiento' || tipo === 'ampliacion') && (
          <label>
            Motivo
            <input
              className="input ml-1 w-64"
              value={op.motivo}
              onChange={(e) => setOp({ ...op, motivo: e.target.value })}
            />
          </label>
        )}
        {tipo === 'aplazamiento' && (
          <label>
            Plazos mensuales
            <input
              type="number"
              min={1}
              max={36}
              className="input ml-1 w-20"
              value={op.plazos ?? ''}
              onChange={(e) =>
                setOp({ ...op, plazos: e.target.value ? Number(e.target.value) : null })
              }
            />
          </label>
        )}
        <label>
          Lugar
          <input
            className="input ml-1 w-32"
            value={op.lugar}
            onChange={(e) => setOp({ ...op, lugar: e.target.value })}
          />
        </label>
        <button type="button" className="btn-ghost" onClick={() => save(false)}>
          Guardar RTF…
        </button>
        <button type="button" className="btn-ghost" onClick={() => save(true)}>
          Guardar PDF…
        </button>
      </div>
    </div>
  );
}

function Fila({
  x,
  onEdit,
  onChange,
}: {
  x: ExpedienteVista;
  onEdit: () => void;
  onChange: (msg?: string) => void;
}) {
  const e = x.expediente;
  const c = x.calculo;
  return (
    <li className="py-3 text-sm">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span className="font-medium">{titulo(e)}</span>
        <span className="flex items-center gap-2">
          {c.proximo && c.dias !== null && (
            <span className={`pill ${pillDias(c.dias)}`}>{quedan(c.dias)}</span>
          )}
          {c.vencido && <span className="pill pill-red">Plazo terminado</span>}
          <span className="pill pill-stone">{ESTADOS[e.estado]}</span>
        </span>
      </div>
      <p className="text-xs text-stone-600">
        {ORGANISMOS[e.organismo]}
        {e.organo && ` · ${e.organo}`}
        {e.concepto && ` · ${e.concepto}`}
        {c.notificado &&
          ` · ${c.presunta ? 'se da por notificada el' : 'notificada el'} ${formatDate(c.notificado)}`}
      </p>
      {c.proximo && (
        <p className="mt-1">
          {c.proximo.titulo}: hasta el <strong>{formatDate(c.proximo.vence)}</strong>
        </p>
      )}
      <details className="mt-1">
        <summary className="cursor-pointer text-link">Plazos, cálculos y escritos</summary>
        <CalculoView c={c} />
        {x.actuaciones.length > 0 && (
          <ul className="mt-2 space-y-1">
            {x.actuaciones.map((a) => (
              <li key={a.id} className="flex flex-wrap items-center gap-2">
                {formatDate(a.fecha)} · {ACTUACIONES[a.tipo]}
                {a.registro && ` (registro ${a.registro})`}
                <ConfirmAction
                  label="Quitar"
                  ariaLabel={`Quitar la actuación del ${formatDate(a.fecha)}`}
                  question="Se quita la anotación (el estado del expediente no cambia)."
                  confirmLabel="Sí, quitar"
                  onConfirm={async () => {
                    await expedienteActuacionDelete(a.id, true);
                    onChange();
                  }}
                />
              </li>
            ))}
          </ul>
        )}
        <NuevaActuacion id={e.id} onDone={onChange} />
        <Escritos x={x} onMsg={onChange} />
        <div className="mt-2 flex gap-3">
          <button type="button" className="text-link" onClick={onEdit}>
            Editar
          </button>
          <ConfirmAction
            label="Borrar"
            ariaLabel={`Borrar el expediente ${titulo(e)}`}
            question="Se borra el expediente con sus anotaciones (la notificación y los documentos se quedan). Queda en la auditoría."
            confirmLabel="Sí, borrar"
            onConfirm={async () => {
              await expedienteDelete(e.id, true);
              onChange('Expediente borrado.');
            }}
          />
        </div>
      </details>
    </li>
  );
}

const DOCS_EXPEDIENTE = [
  'requerimiento_aeat',
  'liquidacion_aeat',
  'resolucion_aeat',
  'notificacion_aeat',
  'documento_aeat',
];
const DOCS_CERTIFICADO = [
  'certificado_aeat',
  'certificado_seguridad_social',
  'certificado_residencia',
  'certificado_situacion_censal',
];

/** Elegir un documento de Documentos AEAT para leer sus datos. */
function DesdeDocumento({
  docs,
  kinds,
  label,
  onPick,
}: {
  docs: CompanyDoc[];
  kinds: string[];
  label: string;
  onPick: (id: number) => void;
}) {
  const opts = docs.filter((d) => kinds.includes(d.kind));
  if (opts.length === 0) return null;
  return (
    <label className="text-sm">
      {label}
      <select
        className="input ml-1"
        value=""
        onChange={(e) => e.target.value && onPick(Number(e.target.value))}
      >
        <option value="">Elige un documento…</option>
        {opts.map((d) => (
          <option key={d.id} value={d.id}>
            {d.title}
            {d.date ? ` (${formatDate(d.date)})` : ''}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Certificados administrativos y apoderamientos con caducidad (G27). */
function Certificados({ docs, today }: { docs: CompanyDoc[]; today: string }) {
  const [list, setList] = useState<CertificadoAdminVista[]>([]);
  const [edit, setEdit] = useState<CertificadoAdmin | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    certificadosAdminList()
      .then(setList)
      .catch(() => setList([]));
  }, []);
  useEffect(load, [load]);
  const nuevo = (): CertificadoAdmin => ({
    id: 0,
    tipo: 'corriente_aeat',
    descripcion: '',
    emitido_el: null,
    caduca_el: null,
    document_id: null,
    notas: '',
  });
  return (
    <section aria-labelledby="cert-admin-h" className="mt-8">
      <h3 id="cert-admin-h" className="font-semibold">
        Certificados y apoderamientos que caducan
      </h3>
      <p className="text-sm text-stone-600">
        Estar al corriente, residencia fiscal, apoderamientos… Anota hasta cuándo valen y TaxPain te
        avisará antes (15 días; 30 los apoderamientos). El certificado electrónico ya avisa desde
        Ajustes.
      </p>
      <ul className="mt-2 divide-y divide-stone-200 text-sm">
        {list.length === 0 && <li className="py-2 text-stone-500">Ninguno anotado.</li>}
        {list.map((c) => (
          <li key={c.certificado.id} className="flex flex-wrap items-center gap-2 py-2">
            <span className="flex-1">
              {CERT_TIPOS[c.certificado.tipo]}
              {c.certificado.descripcion && ` · ${c.certificado.descripcion}`}
            </span>
            {c.certificado.caduca_el && (
              <span
                className={`pill ${c.estado === 'caducado' ? 'pill-red' : c.estado === 'caduca_pronto' ? 'pill-amber' : 'pill-stone'}`}
              >
                {c.estado === 'caducado' ? 'Caducó' : 'Vale hasta'} el{' '}
                {formatDate(c.certificado.caduca_el)}
              </span>
            )}
            <button type="button" className="text-link" onClick={() => setEdit(c.certificado)}>
              Editar
            </button>
            <ConfirmAction
              label="Borrar"
              ariaLabel={`Borrar ${CERT_TIPOS[c.certificado.tipo]}`}
              question="Se borra la anotación (el documento, si lo hay, se queda)."
              confirmLabel="Sí, borrar"
              onConfirm={async () => {
                await certificadoAdminDelete(c.certificado.id, true);
                load();
              }}
            />
          </li>
        ))}
      </ul>
      {!edit && (
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <button type="button" className="btn-ghost" onClick={() => setEdit(nuevo())}>
            Anotar un certificado
          </button>
          <DesdeDocumento
            docs={docs}
            kinds={DOCS_CERTIFICADO}
            label="o desde Documentos AEAT"
            onPick={(id) =>
              certificadoAdminDesdeDocumento(id)
                .then(setEdit)
                .catch((e) => setMsg(errorMessage(e)))
            }
          />
        </div>
      )}
      {edit && (
        <form
          aria-label="Certificado"
          className="card mt-2 grid gap-3 p-4 sm:grid-cols-2"
          onSubmit={async (ev) => {
            ev.preventDefault();
            try {
              await certificadoAdminSave(edit);
              setEdit(null);
              setMsg(null);
              load();
            } catch (e) {
              setMsg(errorMessage(e));
            }
          }}
        >
          <label className="block text-sm">
            Qué es
            <select
              className="input mt-1 w-full"
              value={edit.tipo}
              onChange={(e) => setEdit({ ...edit, tipo: e.target.value as TipoCertificadoAdmin })}
            >
              {Object.entries(CERT_TIPOS).map(([k, v]) => (
                <option key={k} value={k}>
                  {v}
                </option>
              ))}
            </select>
          </label>
          <label className="block text-sm">
            Descripción
            <input
              className="input mt-1 w-full"
              value={edit.descripcion}
              onChange={(e) => setEdit({ ...edit, descripcion: e.target.value })}
            />
          </label>
          {fechaInput('Emitido el', edit.emitido_el, (v) => setEdit({ ...edit, emitido_el: v }))}
          {fechaInput('Vale hasta el', edit.caduca_el, (v) => setEdit({ ...edit, caduca_el: v }))}
          {edit.notas && <p className="text-xs text-stone-600 sm:col-span-2">{edit.notas}</p>}
          <div className="form-actions sm:col-span-2">
            <button type="submit" className="btn-primary">
              Guardar
            </button>
            <button type="button" className="btn-ghost" onClick={() => setEdit(null)}>
              Cancelar
            </button>
          </div>
        </form>
      )}
      {msg && (
        <p role="status" className="mt-2 text-sm">
          {msg}
        </p>
      )}
      <p className="sr-only">Hoy: {today}</p>
    </section>
  );
}

/**
 * Expedientes: requerimientos, propuestas, liquidaciones, sanciones, apremios y embargos de la
 * AEAT o de otros buzones, con sus plazos, los recargos, las reducciones y los borradores de
 * escritos. `borrador`: uno a medio crear desde una notificación del buzón.
 */
export function Expedientes({
  today,
  borrador,
  onBorradorUsado,
}: {
  today: string;
  borrador?: Expediente | null;
  onBorradorUsado?: () => void;
}) {
  const [list, setList] = useState<ExpedienteVista[]>([]);
  const [docs, setDocs] = useState<CompanyDoc[]>([]);
  const [form, setForm] = useState<Expediente | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(() => {
    expedientesList()
      .then(setList)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  useEffect(() => {
    companyDocsList()
      .then(setDocs)
      .catch(() => setDocs([]));
  }, []);
  useEffect(() => {
    if (borrador) {
      setForm(borrador);
      onBorradorUsado?.();
    }
  }, [borrador, onBorradorUsado]);
  const abiertos = list.filter((x) => x.expediente.estado !== 'cerrado');
  const cerrados = list.filter((x) => x.expediente.estado === 'cerrado');
  const urgentes = abiertos.filter((x) => x.calculo.dias !== null && x.calculo.dias <= 7).length;
  return (
    <section aria-labelledby="exp-h" className="mt-8">
      <h2 id="exp-h" className="text-lg font-semibold">
        Expedientes y plazos
        {urgentes > 0 && (
          <span className="ml-2 rounded bg-amber-100 px-2 text-sm text-amber-900">
            {urgentes} con plazo esta semana
          </span>
        )}
      </h2>
      <p className="mt-1 text-sm text-stone-600">
        Requerimientos, propuestas, liquidaciones, sanciones y apremios de la AEAT, la DEHú, la
        Seguridad Social u otras sedes. TaxPain cuenta los plazos con tus festivos y te avisa; nada
        se presenta ni se envía desde aquí.
      </p>
      {!form && (
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <button type="button" className="btn-ghost" onClick={() => setForm(vacio())}>
            Nuevo expediente
          </button>
          <DesdeDocumento
            docs={docs}
            kinds={DOCS_EXPEDIENTE}
            label="o desde Documentos AEAT"
            onPick={(id) =>
              expedienteDesdeDocumento(id)
                .then(setForm)
                .catch((e) => setMsg(errorMessage(e)))
            }
          />
        </div>
      )}
      {form && (
        <ExpedienteForm
          initial={form}
          onCancel={() => setForm(null)}
          onSaved={() => {
            setForm(null);
            setMsg('Expediente guardado.');
            load();
          }}
        />
      )}
      {msg && (
        <p role="status" className="mt-2 text-sm">
          {msg}
        </p>
      )}
      <ul className="mt-3 divide-y divide-stone-200">
        {abiertos.length === 0 && (
          <li className="py-3 text-sm text-stone-500">Ningún expediente abierto.</li>
        )}
        {abiertos.map((x) => (
          <Fila
            key={x.expediente.id}
            x={x}
            onEdit={() => setForm(x.expediente)}
            onChange={(m) => {
              if (m) setMsg(m);
              load();
            }}
          />
        ))}
      </ul>
      {cerrados.length > 0 && (
        <details className="mt-3">
          <summary className="cursor-pointer text-sm text-stone-500">
            Cerrados ({cerrados.length})
          </summary>
          <ul className="divide-y divide-stone-200">
            {cerrados.map((x) => (
              <Fila
                key={x.expediente.id}
                x={x}
                onEdit={() => setForm(x.expediente)}
                onChange={(m) => {
                  if (m) setMsg(m);
                  load();
                }}
              />
            ))}
          </ul>
        </details>
      )}
      <Certificados docs={docs} today={today} />
    </section>
  );
}

/** Plazos de los expedientes abiertos que vencen en `year` (para el calendario). */
export function plazosDelAño(
  list: ExpedienteVista[],
  year: number,
): { key: string; titulo: string; vence: string }[] {
  return list
    .filter((x) => x.expediente.estado !== 'cerrado')
    .flatMap((x) =>
      x.calculo.plazos
        .filter((p) => p.vence.startsWith(String(year)))
        .map((p) => ({
          key: `${x.expediente.id}-${p.clave}`,
          titulo: `${titulo(x.expediente)}: ${p.titulo}`,
          vence: p.vence,
        })),
    )
    .sort((a, b) => a.vence.localeCompare(b.vence));
}
