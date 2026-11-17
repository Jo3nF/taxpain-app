import { ClipboardList, ExternalLink } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import {
  type CensoTecleado,
  type CotejoCenso,
  censoCotejo,
  censoGuardar,
  censoGuia,
  censoPresentada,
  censoProgramar,
  censoQuitar,
  type DiferenciaCenso,
  errorMessage,
  formatDate,
  type GuiaCenso,
  openSede,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const OBLIGACIONES: [string, string][] = [
  ['303', 'IVA (303)'],
  ['111', 'Retenciones de trabajo y profesionales (111)'],
  ['115', 'Retenciones de alquileres (115)'],
  ['123', 'Retenciones del capital mobiliario (123)'],
  ['349', 'Operaciones intracomunitarias (349)'],
  ['130', 'Pagos fraccionados del IRPF (130/131)'],
  ['200', 'Impuesto sobre Sociedades (200/202)'],
];

const GRAVEDAD: Record<DiferenciaCenso['gravedad'], [string, string]> = {
  alta: ['pill-red', 'Comunicar'],
  media: ['pill-amber', 'Revisar'],
  info: ['pill-stone', 'Aviso'],
};

function siNo(v: boolean | null): string {
  if (v === null) return '—';
  return v ? 'Sí' : 'No';
}

/** Lista separada por comas → valores sin vacíos. */
function lista(s: string): string[] {
  return s
    .split(/[,;\n]/)
    .map((x) => x.trim())
    .filter(Boolean);
}

function vacio(): CensoTecleado {
  return {
    fecha: null,
    obligaciones: [],
    epigrafes: [],
    roi: null,
    sii: null,
    criterio_caja: null,
    domicilio_fiscal: null,
  };
}

function Tribool({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean | null;
  onChange: (v: boolean | null) => void;
}) {
  return (
    <label>
      {label}
      <select
        value={value === null ? '' : value ? 'si' : 'no'}
        onChange={(e) => onChange(e.target.value === '' ? null : e.target.value === 'si')}
      >
        <option value="">No lo sé</option>
        <option value="si">Sí consta</option>
        <option value="no">No consta</option>
      </select>
    </label>
  );
}

/** Formulario de lo que consta en el censo («Mis datos censales» o el certificado). */
function Teclear({
  cotejo,
  onSaved,
  onCancel,
}: {
  cotejo: CotejoCenso;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const s = cotejo.situacion;
  const [t, setT] = useState<CensoTecleado>(
    () =>
      cotejo.tecleado ?? {
        ...vacio(),
        fecha: s.fecha,
        obligaciones: s.obligaciones,
        epigrafes: s.epigrafes,
        roi: s.roi,
        domicilio_fiscal: s.domicilio_fiscal,
      },
  );
  const [epigrafes, setEpigrafes] = useState(t.epigrafes.join(', '));
  const [actividad, setActividad] = useState(cotejo.epigrafes_actividad.join(', '));
  const [error, setError] = useState<string | null>(null);
  const toggle = (o: string) =>
    setT((x) => ({
      ...x,
      obligaciones: x.obligaciones.includes(o)
        ? x.obligaciones.filter((y) => y !== o)
        : [...x.obligaciones, o],
    }));
  const save = (tecleado: CensoTecleado | null) =>
    censoGuardar(tecleado, lista(actividad))
      .then(onSaved)
      .catch((e) => setError(errorMessage(e)));
  return (
    <form
      className="card mt-4 p-4"
      aria-label="Lo que consta en el censo"
      onSubmit={(e) => {
        e.preventDefault();
        void save({ ...t, epigrafes: lista(epigrafes) });
      }}
    >
      <fieldset>
        <legend className="font-medium">Obligaciones periódicas que constan</legend>
        <div className="mt-2 grid gap-1 sm:grid-cols-2">
          {OBLIGACIONES.map(([o, n]) => (
            <label key={o} className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={t.obligaciones.includes(o)}
                onChange={() => toggle(o)}
              />
              {n}
            </label>
          ))}
        </div>
      </fieldset>
      <div className="form-grid mt-4">
        <label>
          Fecha de los datos
          <input
            type="date"
            value={t.fecha ?? ''}
            onChange={(e) => setT({ ...t, fecha: e.target.value || null })}
          />
        </label>
        <label>
          Epígrafes del IAE que constan (separados por comas)
          <input value={epigrafes} onChange={(e) => setEpigrafes(e.target.value)} />
        </label>
        <Tribool label="Alta en el ROI" value={t.roi} onChange={(roi) => setT({ ...t, roi })} />
        <Tribool label="SII" value={t.sii} onChange={(sii) => setT({ ...t, sii })} />
        <Tribool
          label="Criterio de caja del IVA"
          value={t.criterio_caja}
          onChange={(criterio_caja) => setT({ ...t, criterio_caja })}
        />
        <label>
          Domicilio fiscal que consta
          <input
            value={t.domicilio_fiscal ?? ''}
            onChange={(e) => setT({ ...t, domicilio_fiscal: e.target.value || null })}
          />
        </label>
        <label className="span-2">
          Epígrafes de lo que la empresa hace de verdad (para cotejar los del censo)
          <input value={actividad} onChange={(e) => setActividad(e.target.value)} />
        </label>
      </div>
      {error && (
        <p role="alert" className="mt-3 text-sm text-red-800">
          {error}
        </p>
      )}
      <div className="mt-4 flex flex-wrap gap-2">
        <button type="submit" className="btn-primary">
          Guardar
        </button>
        {cotejo.tecleado && (
          <button type="button" className="btn-ghost" onClick={() => void save(null)}>
            Volver a lo del certificado
          </button>
        )}
        <button type="button" className="btn-ghost" onClick={onCancel}>
          Cancelar
        </button>
      </div>
    </form>
  );
}

/** Asistente «Quiero comunicar…»: apartados del 036, plazo y aviso en el calendario. */
function Asistente({
  cotejo,
  elegido,
  onElegir,
  onProgramado,
}: {
  cotejo: CotejoCenso;
  elegido: string;
  onElegir: (id: string) => void;
  onProgramado: () => void;
}) {
  const [fecha, setFecha] = useState('');
  const [nota, setNota] = useState('');
  const [guia, setGuia] = useState<GuiaCenso | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hecho, setHecho] = useState<string | null>(null);

  useEffect(() => {
    setHecho(null);
    if (!elegido) {
      setGuia(null);
      return;
    }
    censoGuia(elegido, fecha || null)
      .then((g) => {
        setGuia(g);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [elegido, fecha]);

  const grupos = [...new Set(cotejo.supuestos.map((s) => s.grupo))];
  return (
    <section aria-labelledby="censo-asistente" className="mt-6">
      <h3 id="censo-asistente" className="font-semibold">
        Quiero comunicar…
      </h3>
      <div className="form-grid mt-2">
        <label>
          Cambio
          <select value={elegido} onChange={(e) => onElegir(e.target.value)}>
            <option value="">Elige qué quieres comunicar</option>
            {grupos.map((g) => (
              <optgroup key={g} label={g}>
                {cotejo.supuestos
                  .filter((s) => s.grupo === g)
                  .map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.titulo}
                    </option>
                  ))}
              </optgroup>
            ))}
          </select>
        </label>
        <label>
          Fecha del hecho (o de la primera operación)
          <input type="date" value={fecha} onChange={(e) => setFecha(e.target.value)} />
        </label>
      </div>
      {error && (
        <p role="alert" className="mt-3 text-sm text-red-800">
          {error}
        </p>
      )}
      {guia && (
        <div className="card mt-4 p-4">
          <h4 className="font-medium">{guia.titulo}</h4>
          <p className="mt-1 text-sm text-stone-700">{guia.resumen}</p>
          <p className="mt-3 text-sm">
            <strong>{guia.por_036 ? 'Causa en el 036: ' : 'Dónde: '}</strong>
            {guia.causa}
          </p>
          <ol className="mt-2 list-decimal space-y-1 pl-5 text-sm">
            {guia.pasos.map((p) => (
              <li key={p.apartado}>
                <strong>{p.apartado}.</strong> {p.que_hacer}
              </li>
            ))}
          </ol>
          <p className="mt-3 text-sm">
            <strong>Plazo: </strong>
            {guia.plazo.texto}
          </p>
          {guia.documentacion.length > 0 && (
            <ul className="mt-2 list-disc pl-5 text-sm">
              {guia.documentacion.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          )}
          {guia.avisos.length > 0 && (
            <ul className="mt-2 list-disc pl-5 text-sm text-stone-700">
              {guia.avisos.map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
          <p className="mt-2 text-xs text-stone-500">
            {guia.norma}. {guia.verificar}
          </p>
          <label className="mt-3 block text-sm">
            Nota para el aviso (opcional)
            <input value={nota} onChange={(e) => setNota(e.target.value)} />
          </label>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            {guia.sede && (
              <button
                type="button"
                className="btn-ghost"
                onClick={() => guia.sede && openSede(guia.sede).catch(() => {})}
              >
                <ExternalLink size={16} aria-hidden="true" />
                Abrir el 036 en la Sede
              </button>
            )}
            {guia.plazo.vence ? (
              <ConfirmAction
                label="Añadir el plazo al calendario"
                question={`Se apuntará en el calendario «${guia.titulo}» con último día el ${formatDate(guia.plazo.vence)}. No se presenta nada.`}
                confirmLabel="Añadir"
                danger={false}
                className="btn-ghost"
                onConfirm={async () => {
                  await censoProgramar(guia.id, fecha || null, nota);
                  setHecho(
                    `Apuntado en el calendario hasta el ${formatDate(guia.plazo.vence ?? '')}.`,
                  );
                  onProgramado();
                }}
              />
            ) : (
              guia.plazo.pide_fecha && (
                <span className="text-sm text-stone-600">
                  Pon la fecha del hecho para apuntar el plazo.
                </span>
              )
            )}
          </div>
          {hecho && (
            <p role="status" className="mt-2 text-sm text-green-800">
              {hecho}
            </p>
          )}
        </div>
      )}
    </section>
  );
}

/** Censo (modelo 036): cotejo con el perfil y la contabilidad, y asistente de cambios. */
export function Censo() {
  const [cotejo, setCotejo] = useState<CotejoCenso | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [elegido, setElegido] = useState('');

  const load = useCallback(() => {
    censoCotejo()
      .then((c) => {
        setCotejo(c);
        setError(null);
      })
      .catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(load, [load]);

  const s = cotejo?.situacion;
  const pendientes = cotejo?.comunicaciones.filter((c) => !c.hecha) ?? [];
  return (
    <section aria-labelledby="censo-title" className="mt-10">
      <div className="flex items-center gap-3">
        <span className="icon-tile" aria-hidden="true">
          <ClipboardList size={18} />
        </span>
        <h2 id="censo-title" className="text-lg font-semibold">
          Censo (modelo 036)
        </h2>
      </div>
      <p className="mt-1 text-sm text-stone-600">
        Lo que consta en el censo de la AEAT frente a tu perfil fiscal y a lo que dice la
        contabilidad. TaxPain no presenta el 036: te dice qué apartados tocar y el plazo.
      </p>
      {error && (
        <p role="alert" className="mt-3 rounded-md bg-red-50 p-3 text-sm text-red-900">
          {error}
        </p>
      )}
      {cotejo && s && (
        <>
          <div className="mt-3 flex flex-wrap items-center gap-2 text-sm">
            <span>
              {s.origen === 'tecleado' && 'Según lo que has tecleado'}
              {s.origen === 'certificado' &&
                `Según ${s.documento ?? 'el certificado de situación censal'}`}
              {s.origen === 'ninguno' && 'Sin datos del censo'}
              {s.fecha && ` (${formatDate(s.fecha)})`}
            </span>
            {!editing && (
              <button type="button" className="btn-ghost" onClick={() => setEditing(true)}>
                Teclear lo que consta
              </button>
            )}
          </div>
          {editing && (
            <Teclear
              cotejo={cotejo}
              onCancel={() => setEditing(false)}
              onSaved={() => {
                setEditing(false);
                load();
              }}
            />
          )}
          <div className="mt-4 overflow-x-auto">
            <table className="w-full text-sm">
              <caption className="sr-only">Obligaciones: censo, perfil y contabilidad</caption>
              <thead>
                <tr className="text-left text-stone-500">
                  <th scope="col">Obligación</th>
                  <th scope="col">Censo</th>
                  <th scope="col">Perfil</th>
                  <th scope="col">Contabilidad</th>
                  <th scope="col">Presentado</th>
                </tr>
              </thead>
              <tbody>
                {cotejo.filas.map((f) => (
                  <tr key={f.codigo} className="border-t border-stone-200">
                    <th scope="row" className="py-1 text-left font-normal">
                      {f.nombre}
                    </th>
                    <td>{siNo(f.censo)}</td>
                    <td>{siNo(f.perfil)}</td>
                    <td>{f.contabilidad ?? '—'}</td>
                    <td>{f.presentado.length ? f.presentado.join(', ') : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="mt-2 text-sm text-stone-600">
            Epígrafes: {s.epigrafes.length ? s.epigrafes.join(', ') : '—'} · ROI: {siNo(s.roi)}
            {s.domicilio_fiscal && ` · Domicilio fiscal: ${s.domicilio_fiscal}`}
          </p>

          <section aria-labelledby="censo-dif" className="mt-6">
            <h3 id="censo-dif" className="font-semibold">
              Diferencias ({cotejo.diferencias.length})
            </h3>
            {cotejo.diferencias.length === 0 && (
              <p className="mt-2 text-sm text-stone-600">
                El censo, el perfil y la contabilidad coinciden.
              </p>
            )}
            <ul className="mt-2 divide-y divide-stone-200">
              {cotejo.diferencias.map((d) => (
                <li key={d.clave} className="py-3">
                  <p className="font-medium">
                    <span className={`pill ${GRAVEDAD[d.gravedad][0]} mr-2`}>
                      {GRAVEDAD[d.gravedad][1]}
                    </span>
                    {d.titulo}
                  </p>
                  <p className="mt-1 text-sm text-stone-700">{d.detalle}</p>
                  <p className="mt-1 text-sm">{d.que_hacer}</p>
                  {d.supuesto && (
                    <button
                      type="button"
                      className="btn-ghost mt-1 text-sm"
                      onClick={() => {
                        setElegido(d.supuesto ?? '');
                        document
                          .getElementById('censo-asistente')
                          ?.scrollIntoView?.({ behavior: 'smooth' });
                      }}
                    >
                      Ver qué marcar en el 036
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </section>

          <Asistente cotejo={cotejo} elegido={elegido} onElegir={setElegido} onProgramado={load} />

          {pendientes.length > 0 && (
            <section aria-labelledby="censo-pend" className="mt-6">
              <h3 id="censo-pend" className="font-semibold">
                Cambios apuntados en el calendario
              </h3>
              <ul className="mt-2 divide-y divide-stone-200">
                {pendientes.map((c) => (
                  <li key={c.id} className="flex flex-wrap items-center gap-3 py-2 text-sm">
                    <span className="flex-1">
                      {c.titulo} · hasta el {formatDate(c.vence)}
                      {c.nota && ` · ${c.nota}`}
                    </span>
                    <ConfirmAction
                      label="Ya lo he presentado"
                      ariaLabel={`Ya lo he presentado: ${c.titulo}`}
                      question={`¿Marcar «${c.titulo}» como presentado? Deja de salir en el calendario.`}
                      confirmLabel="Marcar"
                      danger={false}
                      className="btn-ghost"
                      onConfirm={async () => {
                        await censoPresentada(c.id);
                        load();
                      }}
                    />
                    <ConfirmAction
                      label="Quitar"
                      ariaLabel={`Quitar: ${c.titulo}`}
                      question={`¿Quitar el aviso «${c.titulo}» del calendario?`}
                      confirmLabel="Quitar"
                      onConfirm={async () => {
                        await censoQuitar(c.id, true);
                        load();
                      }}
                    />
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </section>
  );
}
