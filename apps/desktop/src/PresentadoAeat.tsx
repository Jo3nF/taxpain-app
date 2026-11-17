import { useEffect, useRef, useState } from 'react';
import {
  type AeatCheckProgress,
  type AeatCheckReport,
  type AeatCheckRow,
  type AeatCheckTarget,
  type AeatYearSummary,
  aeatCheckReport,
  aeatCheckRunning,
  aeatCheckSave,
  aeatCheckSaveYears,
  aeatCheckSummaries,
  aeatCheckTarget,
  aeatCheckYear,
  aeatCheckYears,
  errorMessage,
  formatDate,
  onAeatCheckDone,
  onAeatCheckProgress,
  pickFolder,
} from './api';
import { CertificateSelect } from './CertificateSelect';
import { useInlineDialog, useReturnFocus } from './dialogs';

const PERIOD_NAMES: Record<string, string> = { '0A': 'Anual' };

function periodName(p: string): string {
  return PERIOD_NAMES[p] ?? p;
}

function statusText(r: AeatCheckRow): string {
  if (r.status === 'Missing') return 'Falta';
  if (r.status === 'Open') return 'En plazo';
  if (r.status === 'NotFound') return 'No consta en la AEAT';
  return r.late ? 'Presentada fuera de plazo' : 'Presentada';
}

function statusClass(r: AeatCheckRow): string {
  if (r.status === 'Missing') return 'text-red-700 font-medium';
  if (r.status === 'Open' || r.status === 'NotFound') return 'text-stone-600';
  return r.late ? 'text-amber-800' : 'text-emerald-800';
}

function comparison(r: AeatCheckRow): string {
  if (r.status !== 'Presented') return '';
  if (r.mismatches === 0) return 'Cuadra con TaxPain';
  if (r.mismatches !== null) {
    return r.mismatches === 1
      ? '1 casilla no cuadra con TaxPain'
      : `${r.mismatches} casillas no cuadran con TaxPain`;
  }
  return r.boxes_read ? 'Casillas leídas' : '';
}

function day(s: string | null): string {
  return s ? formatDate(s.slice(0, 10)) : '';
}

function plural(n: number, one: string, many: string): string {
  return n === 1 ? one : many.replace('#', String(n));
}

/** Titular del resumen: no promete más de lo que se sabe. */
function headline(r: AeatCheckReport): string {
  if (r.without_profile) return 'Sin perfil fiscal no se puede saber qué falta.';
  const missing = r.rows.filter((x) => x.status === 'Missing').length;
  const failed = r.errors.length;
  const prefix = failed
    ? plural(failed, 'No se pudo consultar 1 modelo. ', 'No se pudieron consultar # modelos. ')
    : '';
  if (missing === 0) {
    return failed
      ? `${prefix}De los demás, no falta ninguna declaración de las que tocaban seguro.`
      : 'No falta ninguna declaración de las que tocaban seguro.';
  }
  return (
    prefix +
    plural(
      missing,
      'Falta 1 declaración de las que tocaban.',
      'Faltan # declaraciones de las que tocaban.',
    )
  );
}

/** El modelo de un error guardado como «modelo: mensaje». */
function modelOf(e: string): string {
  return e.split(':')[0]?.trim() ?? e;
}

/** Avisos de un año en el resumen: qué no se pudo consultar y por qué. */
function yearNotes(s: AeatYearSummary): string[] {
  const out: string[] = [];
  if (s.failed.length) {
    out.push(
      `${plural(s.failed.length, 'No se pudo consultar el', 'No se pudieron consultar los')} ${s.failed
        .map(modelOf)
        .join(', ')} (la AEAT dio un error: se reintenta al volver a comprobar)`,
    );
  }
  if (s.unavailable.length) {
    out.push(
      `${s.unavailable.join(', ')}: este servicio de la AEAT no ${
        s.unavailable.length === 1 ? 'lo' : 'los'
      } deja consultar; míralo en la Sede`,
    );
  }
  const unread = s.presented - s.boxes_read;
  if (unread > 0) {
    out.push(
      plural(unread, '1 declaración sin casillas leídas', '# declaraciones sin casillas leídas'),
    );
  }
  return out;
}

type Pending = { kind: 'year' } | { kind: 'years'; from: number; to: number };

/**
 * «Comprobar lo presentado»: consulta en la AEAT, con el certificado, las declaraciones de la
 * empresa de los modelos habituales y de los del perfil fiscal (también las de la gestoría),
 * descarga sus justificantes y dice qué falta y qué no cuadra con lo que calcula TaxPain. Se
 * puede comprobar un año o varios con una sola confirmación. Sólo lectura.
 *
 * Arriba, el resumen de todos los años comprobados (con lo que significa cada columna); debajo,
 * el detalle del año elegido, que empieza siendo el del «Ejercicio» de Modelos.
 */
export function PresentadoAeat({ year }: { year: number }) {
  const thisYear = new Date().getFullYear();
  const [detailYear, setDetailYear] = useState(year);
  const [report, setReport] = useState<AeatCheckReport | null>(null);
  const [summaries, setSummaries] = useState<AeatYearSummary[]>([]);
  const [yearErrors, setYearErrors] = useState<Record<number, string>>({});
  const [cert, setCert] = useState('');
  const [pending, setPending] = useState<Pending | null>(null);
  const [target, setTarget] = useState<AeatCheckTarget | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<AeatCheckProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  // Lo guardado del año que se ve: cargando, o el error al leerlo.
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rangeOpen, setRangeOpen] = useState(false);
  const [from, setFrom] = useState(Math.max(2000, thisYear - 4));
  // El año que se ve: lo que llega tarde de otro año no se pinta bajo éste.
  const yearRef = useRef(detailYear);
  yearRef.current = detailYear;
  const trigger = useRef<HTMLButtonElement | null>(null);
  const mainBtn = useRef<HTMLButtonElement>(null);
  const rangeBtn = useRef<HTMLButtonElement>(null);
  const detailRef = useRef<HTMLHeadingElement>(null);
  const dialog = useInlineDialog(pending !== null, busy, () => setPending(null));
  // Al cerrar la confirmación o acabar la consulta, el foco vuelve al botón que la abrió.
  useReturnFocus(pending !== null || busy, trigger);

  // El «Ejercicio» de Modelos manda el año del detalle.
  useEffect(() => {
    setDetailYear(year);
  }, [year]);

  useEffect(() => {
    let alive = true;
    setReport(null);
    setMsg(null);
    setError(null);
    setLoading(true);
    setLoadError(null);
    Promise.resolve()
      .then(() => aeatCheckReport(detailYear))
      .then((r) => {
        if (!alive) return;
        setReport(r ?? null);
        setLoading(false);
      })
      .catch((e) => {
        if (!alive) return;
        setReport(null);
        setLoadError(errorMessage(e));
        setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [detailYear]);

  const loadSummaries = () =>
    Promise.resolve()
      .then(() => aeatCheckSummaries())
      .then((s) => setSummaries(Array.isArray(s) ? s : []))
      .catch(() => setSummaries([]));

  // biome-ignore lint/correctness/useExhaustiveDependencies: sólo al abrir
  useEffect(() => {
    loadSummaries();
    // Si se salió de Modelos con una comprobación en marcha, al volver se espera a que acabe.
    let alive = true;
    let stops: Array<() => void> = [];
    Promise.resolve()
      .then(() => aeatCheckRunning())
      .then(async (running) => {
        if (!alive || !running) return;
        setBusy(true);
        stops = [
          await onAeatCheckProgress(setProgress),
          await onAeatCheckDone(() => {
            setBusy(false);
            setProgress(null);
            loadSummaries();
            const shown = yearRef.current;
            aeatCheckReport(shown)
              .then((r) => yearRef.current === shown && setReport(r))
              .catch(() => {});
          }),
        ];
      })
      .catch(() => {});
    return () => {
      alive = false;
      for (const s of stops) s();
    };
  }, []);

  const ask = async (p: Pending, button: HTMLButtonElement | null) => {
    setError(null);
    setMsg(null);
    trigger.current = button;
    if (!cert) {
      setError('Importa primero el certificado de la empresa en Ajustes.');
      return;
    }
    try {
      const t = await aeatCheckTarget(cert);
      if (!t.company_nif) {
        setError(
          'Pon el NIF de la empresa (en Facturación o en la ficha de la empresa) antes de comprobar lo presentado.',
        );
        return;
      }
      if (t.other_entity) {
        setError(
          `El certificado es de representante de ${t.cert_nif} y la empresa abierta es ${t.company_nif}: elige el de esta empresa.`,
        );
        return;
      }
      setTarget(t);
      setPending(p);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const run = async () => {
    const p = pending;
    const other = target ? !target.same : false;
    setPending(null);
    if (!p) return;
    setBusy(true);
    setProgress(null);
    const stop = await onAeatCheckProgress(setProgress);
    try {
      if (p.kind === 'year') {
        const asked = detailYear;
        const r = await aeatCheckYear(asked, cert, other);
        // Comprobado bien: su error de «varios años», si lo tenía, ya no vale.
        setYearErrors((e) => {
          const { [asked]: _, ...rest } = e;
          return rest;
        });
        if (yearRef.current === asked) {
          setReport(r);
          setLoadError(null);
        }
      } else {
        const outcomes = await aeatCheckYears(p.from, p.to, cert, other);
        const errs: Record<number, string> = {};
        for (const o of outcomes) if (o.error) errs[o.year] = o.error;
        setYearErrors(errs);
        const shown = yearRef.current;
        const r = await aeatCheckReport(shown);
        if (yearRef.current === shown) {
          setReport(r);
          setLoadError(null);
        }
      }
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      stop();
      setProgress(null);
      setBusy(false);
      loadSummaries();
    }
  };

  const save = async (years?: { from: number; to: number }) => {
    setError(null);
    setMsg(null);
    try {
      const dir = await pickFolder();
      if (!dir) return;
      const paths = years
        ? await aeatCheckSaveYears(years.from, years.to, dir)
        : await aeatCheckSave(detailYear, dir);
      setMsg(
        `${plural(paths.length, '1 justificante guardado', '# justificantes guardados')} en ${dir}${
          years ? ', cada año en su carpeta' : ''
        }.`,
      );
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const showYear = (y: number) => {
    setDetailYear(y);
    // Al abrir un año, el foco va a su detalle (que se pinta debajo).
    setTimeout(() => detailRef.current?.focus(), 0);
  };

  const visible = report?.year === detailYear ? report : null;
  const checked = visible?.checked_at != null;
  const pdfs = visible?.rows.filter((r) => r.has_pdf).length ?? 0;
  const notFound = visible?.rows.filter((r) => r.status === 'NotFound').length ?? 0;
  const yearsOk = from >= 2000 && from <= thisYear && thisYear - from < 15;
  const failedYears = Object.keys(yearErrors)
    .map(Number)
    .filter((y) => !summaries.some((s) => s.year === y));
  const rows = [
    ...summaries.map((s) => ({ year: s.year, s })),
    ...failedYears.map((y) => ({ year: y, s: null as AeatYearSummary | null })),
  ].sort((a, b) => b.year - a.year);
  const years = rows.map((r) => r.year);
  const range = years.length ? { from: Math.min(...years), to: Math.max(...years) } : null;

  return (
    <section aria-labelledby="aeat-check-h" className="mt-8 text-sm">
      <h3 id="aeat-check-h" className="font-medium">
        Comprobar lo presentado en la AEAT
      </h3>
      <p className="mt-1 text-stone-600">
        TaxPain pregunta a la AEAT, con tu certificado, qué declaraciones tiene de la empresa (los
        modelos habituales y los de tu perfil fiscal), también las que presentó la gestoría.
        Descarga sus justificantes, lee sus casillas, dice qué falta y las compara con lo que
        calcula TaxPain. No presenta ni cambia nada.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <button
          ref={mainBtn}
          type="button"
          disabled={busy}
          className="rounded border border-stone-300 px-3 py-1"
          onClick={() => ask({ kind: 'year' }, mainBtn.current)}
        >
          {busy ? 'Consultando la AEAT…' : `Comprobar lo presentado en ${detailYear}`}
        </button>
        <CertificateSelect value={cert} onChange={setCert} />
        <button
          ref={rangeBtn}
          type="button"
          disabled={busy}
          className="text-link"
          aria-expanded={rangeOpen}
          onClick={() => setRangeOpen((o) => !o)}
        >
          Comprobar varios años…
        </button>
      </div>
      {rangeOpen && (
        <div className="mt-3 flex flex-wrap items-end gap-3">
          <label className="flex flex-col">
            <span className="text-stone-600">Desde el año</span>
            <input
              type="number"
              min={2000}
              max={thisYear}
              value={from}
              onChange={(e) => setFrom(Number(e.target.value))}
              className="w-24 rounded border border-stone-300 px-2 py-1"
            />
          </label>
          <span className="pb-1 text-stone-600">hasta {thisYear}</span>
          <button
            type="button"
            disabled={busy || !yearsOk}
            className="rounded border border-stone-300 px-3 py-1"
            onClick={() => ask({ kind: 'years', from, to: thisYear }, rangeBtn.current)}
          >
            Comprobar de {from} a {thisYear}
          </button>
          {!yearsOk && (
            <span className="pb-1 text-amber-800">
              Elige un año entre 2000 y {thisYear}, hasta 15 años.
            </span>
          )}
        </div>
      )}
      {pending && (
        <div
          ref={dialog.ref}
          onKeyDown={dialog.onKeyDown}
          role="dialog"
          aria-label="Comprobar en la AEAT"
          className="mt-3 space-y-2 rounded-lg border border-amber-300 bg-amber-50 p-3"
        >
          <p>
            Se usará tu certificado para consultar en la AEAT las declaraciones de{' '}
            {pending.kind === 'year' ? detailYear : `${pending.from} a ${pending.to}`} y descargar
            sus justificantes. Es sólo una consulta: no se presenta nada.
            {pending.kind === 'years' && ' Puede tardar varios minutos.'}
          </p>
          {target && !target.same && (
            <p className="font-medium">
              El certificado es de {target.cert_name || target.cert_nif} ({target.cert_nif}), no de
              la empresa ({target.company_nif}). Sólo sirve si está apoderado o es colaborador
              social de la empresa en la AEAT: se consultará con el NIF de la empresa y la AEAT
              decidirá si lo admite.
            </p>
          )}
          <div className="flex gap-3">
            <button type="button" className="btn-primary" onClick={run}>
              {target && !target.same ? 'Consultar con este certificado' : 'Consultar'}
            </button>
            <button type="button" className="text-link" onClick={() => setPending(null)}>
              Cancelar
            </button>
          </div>
        </div>
      )}
      {busy && (
        <p role="status" aria-live="polite" className="mt-2 text-stone-600">
          {progress
            ? `${progress.year}: ${progress.step === 'consulta' ? 'consultando modelos' : 'descargando justificantes y leyendo casillas'} (${progress.done} de ${progress.total})…`
            : 'Consultando la AEAT…'}
        </p>
      )}
      {error && (
        <p role="alert" className="mt-2 text-red-700">
          {error}
        </p>
      )}
      {msg && (
        <p role="status" className="mt-2">
          {msg}
        </p>
      )}
      {rows.length > 0 && (
        <div className="mt-4 space-y-2">
          <h4 className="font-medium">Resumen por años</h4>
          <table className="w-full" aria-label="Resumen por años">
            <thead>
              <tr className="text-left text-stone-500">
                <th className="py-1 font-normal">Año</th>
                <th className="font-normal">Presentadas</th>
                <th className="font-normal">Fuera de plazo</th>
                <th className="font-normal">Faltan</th>
                <th className="font-normal">No constan</th>
                <th className="font-normal">Justificantes</th>
                <th className="font-normal">Avisos</th>
                <th className="font-normal">
                  <span className="sr-only">Detalle</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map(({ year: y, s }) => (
                <tr
                  key={y}
                  className={`border-t border-stone-100 align-top ${
                    y === detailYear ? 'bg-stone-50' : ''
                  }`}
                >
                  <td className="py-1 font-medium">{y}</td>
                  <td>{s?.presented ?? ''}</td>
                  <td className={s?.late ? 'text-amber-800' : ''}>{s ? s.late : ''}</td>
                  <td className={s?.missing ? 'text-red-700 font-medium' : ''}>
                    {s?.missing ?? ''}
                  </td>
                  <td>{s?.not_found ?? ''}</td>
                  <td>{s ? `${s.pdfs} de ${s.presented}` : ''}</td>
                  <td className="text-amber-800">
                    {yearErrors[y] && <span className="block">{yearErrors[y]}</span>}
                    {s &&
                      yearNotes(s).map((n) => (
                        <span key={n} className="block">
                          {n}
                        </span>
                      ))}
                  </td>
                  <td className="text-right">
                    {s && (
                      <button
                        type="button"
                        className="text-link"
                        aria-label={`Ver el detalle de ${y}`}
                        aria-pressed={y === detailYear}
                        onClick={() => showYear(y)}
                      >
                        Ver detalle
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <details className="text-stone-600">
            <summary className="cursor-pointer">Qué significa cada columna</summary>
            <dl className="mt-1 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1">
              <dt className="font-medium">Presentadas</dt>
              <dd>
                Declaraciones que tiene la AEAT de ese año, también las de la gestoría. Una
                complementaria cuenta una vez, con la que sustituye.
              </dd>
              <dt className="font-medium">Fuera de plazo</dt>
              <dd>Presentadas después del último día del plazo: pueden llevar recargo.</dd>
              <dt className="font-medium">Faltan</dt>
              <dd>
                Tocaban seguro según el perfil fiscal (o TaxPain calcula importes), el plazo acabó y
                la AEAT no las tiene.
              </dd>
              <dt className="font-medium">No constan</dt>
              <dd>
                La AEAT no las tiene, pero sólo tocaban en ciertos casos (retenciones, operaciones
                intracomunitarias…): mira la nota de cada una en el detalle.
              </dd>
              <dt className="font-medium">Justificantes</dt>
              <dd>Justificantes en PDF descargados, de las presentadas.</dd>
              <dt className="font-medium">Avisos</dt>
              <dd>
                Modelos que no se pudieron consultar (se reintentan al volver a comprobar), los que
                este servicio de la AEAT no deja consultar y declaraciones sin casillas leídas.
              </dd>
            </dl>
          </details>
          {range && summaries.some((s) => s.pdfs > 0) && (
            <button type="button" className="text-link" onClick={() => save(range)}>
              Guardar los justificantes de {range.from} a {range.to} en una carpeta…
            </button>
          )}
        </div>
      )}
      <div className="mt-6 space-y-2">
        <h4 ref={detailRef} tabIndex={-1} className="font-medium">
          Detalle de {detailYear}
        </h4>
        {loading && <p className="text-stone-600">Cargando…</p>}
        {loadError && (
          <p role="alert" className="text-red-700">
            No se pudo leer lo comprobado de {detailYear}: {loadError}
          </p>
        )}
        {!loading && !loadError && !checked && (
          <p className="text-stone-600">
            {detailYear} aún no se ha comprobado con la AEAT: pulsa «Comprobar lo presentado en{' '}
            {detailYear}».
          </p>
        )}
        {visible && checked && (
          <>
            <p role="status">
              {headline(visible)} Consultado el {day(visible.checked_at)}
              {visible.nif ? ` con el NIF ${visible.nif}` : ''}.
            </p>
            {pdfs > 0 && (
              <button type="button" className="text-link" onClick={() => save()}>
                Guardar los justificantes de {detailYear} en una carpeta…
              </button>
            )}
            {notFound > 0 && (
              <p className="text-stone-600">
                {plural(
                  notFound,
                  '1 declaración no consta en la AEAT, pero sólo tocaba en ciertos casos: revisa la nota.',
                  '# declaraciones no constan en la AEAT, pero sólo tocaban en ciertos casos: revisa las notas.',
                )}
              </p>
            )}
            {visible.without_profile ? (
              <p className="text-amber-800">
                Sin perfil fiscal no se sabe qué tocaba presentar: configúralo en Calendario →
                Perfil.
              </p>
            ) : (
              detailYear < thisYear && (
                <p className="text-stone-600">
                  Lo que tocaba sale de tu perfil fiscal de hoy: si ese año tenías otras
                  obligaciones u otra periodicidad, revísalo.
                </p>
              )
            )}
            {visible.rows.length > 0 && (
              <table className="w-full" aria-label={`Declaraciones de ${detailYear}`}>
                <thead>
                  <tr className="text-left text-stone-500">
                    <th className="py-1 font-normal">Modelo</th>
                    <th className="font-normal">Periodo</th>
                    <th className="font-normal">Estado</th>
                    <th className="font-normal">Presentada</th>
                    <th className="font-normal">Justificante</th>
                    <th className="font-normal">Casillas y TaxPain</th>
                  </tr>
                </thead>
                <tbody>
                  {visible.rows.map((r) => (
                    <tr
                      key={`${r.model}-${r.period}-${r.receipt ?? ''}`}
                      className="border-t border-stone-100 align-top"
                    >
                      <td className="py-1">{r.model}</td>
                      <td>{periodName(r.period)}</td>
                      <td className={statusClass(r)}>
                        {statusText(r)}
                        {r.status !== 'Presented' && r.due_on && (
                          <span className="block text-stone-500">
                            plazo hasta el {formatDate(r.due_on)}
                          </span>
                        )}
                        {r.status === 'Presented' && !r.expected && !visible.without_profile && (
                          <span className="block text-stone-500">el perfil no la esperaba</span>
                        )}
                        {r.late_amendment && (
                          <span className="block text-amber-800">
                            complementaria fuera de plazo (puede llevar recargo)
                          </span>
                        )}
                      </td>
                      <td>
                        {day(r.presented_at)}
                        {r.original_presented_at && (
                          <span className="block text-stone-500">
                            la primera, el {day(r.original_presented_at)}
                          </span>
                        )}
                      </td>
                      <td>
                        {r.receipt ?? ''}
                        {r.replaces.length > 0 && (
                          <span className="block text-stone-500">
                            sustituye a {r.replaces.join(', ')}
                          </span>
                        )}
                        {r.status === 'Presented' && !r.has_pdf && (
                          <span className="block text-stone-500">sin PDF</span>
                        )}
                      </td>
                      <td className={r.mismatches ? 'text-amber-800' : ''}>
                        {comparison(r)}
                        {r.note && <span className="block text-stone-500">{r.note}</span>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {visible.none_filed.length > 0 && (
              <p className="text-stone-600">
                Sin declaraciones en la AEAT
                {visible.without_profile ? '' : ' (y no tocaban)'}: {visible.none_filed.join(', ')}.
                {visible.empty_answers[0] &&
                  ` La AEAT contestó «${visible.empty_answers[0].text}».`}
              </p>
            )}
            {visible.unavailable.length > 0 && (
              <p className="text-stone-600">
                Este servicio de la AEAT no deja consultar {visible.unavailable.join(', ')}:
                compruébalo en la Sede.
              </p>
            )}
            {visible.errors.length > 0 && (
              <div role="alert" className="text-amber-800">
                <p>No se pudieron consultar (se reintenta al volver a comprobar):</p>
                <ul className="list-disc pl-5">
                  {visible.errors.map((e) => (
                    <li key={e}>{e}</li>
                  ))}
                </ul>
              </div>
            )}
            {visible.rows.some((r) => r.mismatches) && (
              <p className="text-stone-600">
                El detalle casilla a casilla está en Importar → Comparar con la gestoría.
              </p>
            )}
          </>
        )}
      </div>
    </section>
  );
}
