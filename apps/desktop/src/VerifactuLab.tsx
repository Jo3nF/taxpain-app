import { CircleAlert, CircleCheck, FlaskConical, LoaderCircle, TriangleAlert } from 'lucide-react';
import { type ReactNode, useCallback, useEffect, useState } from 'react';
import {
  errorMessage,
  type LabCompared,
  type LabStatus,
  plural,
  sendStateLabel,
  verifactuLabCancel,
  verifactuLabCreate,
  verifactuLabGenerate,
  verifactuLabQuery,
  verifactuLabReportSave,
  verifactuLabStatus,
  verifactuLabUseCertNif,
  verifactuSend,
} from './api';
import { CertificateSelect } from './CertificateSelect';
import { ConfirmAction } from './ConfirmAction';

function Step({
  n,
  title,
  done,
  children,
}: {
  n: number;
  title: string;
  done: boolean;
  children: ReactNode;
}) {
  return (
    <li className="flex gap-3">
      <span
        className={`grid h-7 w-7 shrink-0 place-items-center rounded-full text-sm font-semibold ${
          done ? 'bg-emerald-50 text-emerald-800' : 'bg-stone-100 text-stone-600'
        }`}
        aria-hidden="true"
      >
        {done ? <CircleCheck size={16} /> : n}
      </span>
      <div className="min-w-0 flex-1 pb-4">
        <p className="font-medium">{title}</p>
        <div className="mt-1 text-sm text-stone-600">{children}</div>
      </div>
    </li>
  );
}

/**
 * Banco de pruebas VERI*FACTU: comprueba, crea la empresa de pruebas, genera facturas, las
 * envía al entorno de pruebas de la AEAT, consulta lo registrado y guarda un informe.
 */
export function VerifactuLab({ onSwitched }: { onSwitched?: () => void }) {
  const [cert, setCert] = useState('');
  const [st, setSt] = useState<LabStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [compared, setCompared] = useState<LabCompared[] | null>(null);

  const load = useCallback(() => {
    verifactuLabStatus(cert || null)
      .then(setSt)
      .catch((e) => setMsg(errorMessage(e)));
  }, [cert]);
  useEffect(load, [load]);

  async function run(label: string, f: () => Promise<string | undefined>) {
    setBusy(label);
    setMsg(null);
    try {
      const m = await f();
      if (m) setMsg(m);
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setBusy(null);
      load();
    }
  }

  if (!st) return null;
  const blocking = st.checks.filter((c) => c.blocking && !c.ok);
  const canSend = st.is_lab && blocking.length === 0;
  const generated = (st.info?.invoices.length ?? 0) > 0;
  const pending = st.records.filter((r) => r.send_status === 'pendiente').length;
  const sent = st.records.some((r) => r.send_status !== 'pendiente');
  const cancelled = st.records.some((r) => r.record_type === 'anulacion');
  const now = new Date();

  const send = async () => {
    const r = await verifactuSend(cert, true);
    return r
      ? `Envío ${sendStateLabel(r.estado_envio)} (${plural(r.registros, 'registro')}). Espera ${r.tiempo_espera_envio} s antes del siguiente.`
      : 'No había nada pendiente de enviar.';
  };

  return (
    <section className="mt-8" aria-labelledby="lab-title">
      <div className="flex items-start gap-3">
        <span className="icon-tile" aria-hidden="true">
          <FlaskConical size={18} />
        </span>
        <div>
          <h2 id="lab-title" className="text-lg font-semibold">
            Pruebas VERI*FACTU
          </h2>
          <p className="text-sm text-stone-600">
            Envía facturas de prueba al <strong>entorno de pruebas</strong> de la AEAT
            (prewww1.aeat.es) y comprueba lo que registra. Nada va a producción ni tiene efectos
            fiscales. Se hace en una empresa aparte para no gastar tu numeración real.
          </p>
        </div>
      </div>

      <div className="card mt-4 p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h3 className="font-semibold">Comprobaciones</h3>
          <div className="flex items-center gap-2">
            <CertificateSelect value={cert} onChange={setCert} />
            <button type="button" className="rounded border px-3" onClick={load}>
              Volver a comprobar
            </button>
          </div>
        </div>
        <ul className="mt-3 space-y-1.5 text-sm">
          {st.checks.map((c) => (
            <li key={c.id} className="flex items-start gap-2">
              {c.ok ? (
                <CircleCheck
                  size={16}
                  className="mt-0.5 shrink-0 text-emerald-600"
                  aria-hidden="true"
                />
              ) : c.blocking ? (
                <CircleAlert
                  size={16}
                  className="mt-0.5 shrink-0 text-red-700"
                  aria-hidden="true"
                />
              ) : (
                <TriangleAlert
                  size={16}
                  className="mt-0.5 shrink-0 text-amber-700"
                  aria-hidden="true"
                />
              )}
              <span>
                <span className="font-medium text-stone-900">{c.label}</span>
                <span className="sr-only">{c.ok ? ': bien' : ': falla'}</span>: {c.detail}
              </span>
            </li>
          ))}
        </ul>
      </div>

      <ol className="card mt-4 p-5">
        <Step n={1} title="Empresa de pruebas" done={st.is_lab}>
          {st.is_lab ? (
            <p>Estás en la empresa de pruebas (series PRU y PRUR, VERI*FACTU en modo pruebas).</p>
          ) : (
            <>
              <p>
                Se crea una empresa nueva con el mismo emisor, en modo pruebas, y la app cambia a
                ella. Después importa ahí tu certificado (Ajustes → Certificado electrónico).
              </p>
              <button
                type="button"
                className="btn-primary mt-2 rounded px-3"
                disabled={busy !== null}
                onClick={() =>
                  run('crear', async () => {
                    await verifactuLabCreate();
                    (onSwitched ?? (() => window.location.reload()))();
                    return undefined;
                  })
                }
              >
                Crear la empresa de pruebas
              </button>
            </>
          )}
          {st.is_lab && st.checks.some((c) => c.id === 'nif' && !c.ok) && (
            <button
              type="button"
              className="mt-2 rounded border px-3"
              onClick={() =>
                run('nif', async () => {
                  await verifactuLabUseCertNif(cert || null);
                  return 'El emisor de pruebas es ahora el titular del certificado.';
                })
              }
            >
              Usar el NIF del certificado
            </button>
          )}
        </Step>
        <Step n={2} title="Facturas de prueba" done={generated}>
          <p>
            Cuatro facturas: completa al 21 %, con dos tipos de IVA y retención, simplificada y una
            rectificativa por diferencias de la primera.
          </p>
          {generated ? (
            <p className="mt-1 font-mono text-xs">{st.info?.invoices.join(' · ')}</p>
          ) : (
            <button
              type="button"
              className="mt-2 rounded border px-3"
              disabled={!st.is_lab || busy !== null}
              onClick={() =>
                run('generar', async () => {
                  const n = await verifactuLabGenerate();
                  return `Emitidas: ${n.join(', ')}.`;
                })
              }
            >
              Generar las facturas de prueba
            </button>
          )}
        </Step>
        <Step n={3} title="Enviar al entorno de pruebas" done={sent && pending === 0}>
          <p>
            Con tu certificado y Touch ID.{' '}
            {pending > 0 ? `${plural(pending, 'registro pendiente', 'registros pendientes')}.` : ''}
          </p>
          {pending > 0 && (
            <div className="mt-2">
              {canSend && cert ? (
                <ConfirmAction
                  label={busy === 'enviar' ? 'Enviando…' : `Enviar ${plural(pending, 'registro')}`}
                  className="btn-primary rounded px-3"
                  danger={false}
                  question="Se enviarán los registros al ENTORNO DE PRUEBAS de la AEAT (prewww1.aeat.es), sin efectos fiscales. Te pedirá Touch ID."
                  confirmLabel="Sí, enviar a pruebas"
                  onConfirm={() => run('enviar', send)}
                />
              ) : (
                <>
                  <button type="button" className="btn-primary rounded px-3" disabled>
                    Enviar {plural(pending, 'registro')}
                  </button>
                  <p className="mt-1 text-xs text-red-700">
                    {!st.is_lab
                      ? 'Sólo se envía desde la empresa de pruebas (paso 1).'
                      : blocking.length > 0
                        ? 'Antes resuelve las comprobaciones marcadas en rojo.'
                        : 'Elige el certificado con el que enviar (o impórtalo en Ajustes).'}
                  </p>
                </>
              )}
            </div>
          )}
        </Step>
        <Step n={4} title="Anular una factura y volver a enviar" done={cancelled}>
          <p>Genera el registro de anulación de la segunda factura (va en un envío aparte).</p>
          {!cancelled && (
            <button
              type="button"
              className="mt-2 rounded border px-3"
              disabled={!sent || busy !== null}
              onClick={() =>
                run(
                  'anular',
                  async () => `Anulada ${await verifactuLabCancel()}: envíala en el paso 3.`,
                )
              }
            >
              Anular la factura 2
            </button>
          )}
        </Step>
        <Step
          n={5}
          title="Consultar en la AEAT lo registrado"
          done={compared?.every((c) => c.ok) ?? false}
        >
          <p>
            Pregunta a la AEAT qué tiene de este mes y lo compara con TaxPain (estado y huella).
            Sólo lectura.
          </p>
          <button
            type="button"
            className="mt-2 rounded border px-3"
            disabled={!sent || !cert || busy !== null}
            onClick={() =>
              run('consultar', async () => {
                const c = await verifactuLabQuery(cert, now.getFullYear(), now.getMonth() + 1);
                setCompared(c);
                return c.every((x) => x.ok)
                  ? 'La AEAT tiene todo como TaxPain.'
                  : 'Hay diferencias: revisa la tabla.';
              })
            }
          >
            {busy === 'consultar' ? 'Consultando…' : 'Consultar'}
          </button>
          {compared && (
            <table className="mt-3 w-full text-sm">
              <thead>
                <tr>
                  <th className="text-left">Factura</th>
                  <th className="text-left">TaxPain</th>
                  <th className="text-left">AEAT</th>
                  <th className="text-left">Huella</th>
                </tr>
              </thead>
              <tbody>
                {compared.map((c) => (
                  <tr key={c.num_serie} className={c.ok ? '' : 'bg-amber-50'}>
                    <td className="font-mono">{c.num_serie}</td>
                    <td>{c.local_status}</td>
                    <td>
                      {c.aeat_status ?? 'no está'}
                      {c.error && <span className="block text-xs text-red-700">{c.error}</span>}
                    </td>
                    <td>{c.huella_ok === null ? '—' : c.huella_ok ? 'coincide' : 'NO coincide'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Step>
        <Step n={6} title="Guardar el informe" done={false}>
          <p>Un resumen en Markdown con estados, códigos de error y CSV (sin datos de clientes).</p>
          <button
            type="button"
            className="mt-2 rounded border px-3"
            onClick={() =>
              run('informe', async () =>
                (await verifactuLabReportSave(cert || null)) ? 'Informe guardado.' : undefined,
              )
            }
          >
            Guardar informe…
          </button>
        </Step>
      </ol>

      {busy && (
        <p role="status" className="mt-3 flex items-center gap-2 text-sm text-stone-600">
          <LoaderCircle size={16} className="animate-spin" aria-hidden="true" />
          Trabajando…
        </p>
      )}
      {msg && (
        <p role="status" className="mt-3 rounded-lg bg-stone-100 p-3 text-sm">
          {msg}
        </p>
      )}

      {st.records.length > 0 && (
        <div className="card mt-4 p-5">
          <h3 className="font-semibold">Registros VERI*FACTU de esta empresa</h3>
          <table className="mt-2 w-full text-sm">
            <thead>
              <tr>
                <th className="text-left">Factura</th>
                <th className="text-left">Registro</th>
                <th className="text-left">Estado</th>
                <th className="text-left">Error</th>
                <th className="text-left">CSV</th>
              </tr>
            </thead>
            <tbody>
              {st.records.map((r) => (
                <tr key={`${r.num_serie}-${r.record_type}-${r.sent_at ?? ''}`}>
                  <td className="font-mono">{r.num_serie}</td>
                  <td>{r.record_type === 'anulacion' ? 'Anulación' : 'Alta'}</td>
                  <td>
                    <span
                      className={`pill ${
                        r.send_status === 'correcto' || r.send_status === 'pruebas'
                          ? 'pill-green'
                          : r.send_status === 'pendiente'
                            ? 'pill-stone'
                            : 'pill-red'
                      }`}
                    >
                      {r.send_status === 'pruebas'
                        ? 'Aceptado en pruebas'
                        : r.send_status.replaceAll('_', ' ')}
                    </span>
                  </td>
                  <td className="text-xs text-red-700">{r.error}</td>
                  <td className="font-mono text-xs">{r.csv}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
