import { Archive, FileArchive, Package } from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  type ArchivoHecho,
  type AvisoConservacion,
  archivoAviso,
  archivoConservacion,
  archivoEjercicioContenido,
  archivoEjercicioExportar,
  type Conservacion,
  type ExportacionCompleta,
  errorMessage,
  exportacionCompleta,
  fileSize,
  type ParteArchivo,
  pickZipSave,
} from './api';

const BUTTON = 'rounded-md border border-stone-300 bg-white px-3 py-1.5 text-sm';
const PRIMARY = 'rounded-md bg-stone-900 px-3 py-1.5 text-sm text-white disabled:opacity-60';

function fecha(d: string | null): string {
  if (!d) return 'mientras queden créditos pendientes';
  const [y, m, day] = d.split('-');
  return `${day}/${m}/${y}`;
}

const MARCA: Record<ParteArchivo['estado'], string> = {
  si: '✓',
  parcial: '~',
  no: '✗',
};
const ESTADO: Record<ParteArchivo['estado'], string> = {
  si: 'Está',
  parcial: 'Incompleto',
  no: 'Falta',
};

/** Aviso de que el ZIP va sin cifrar, con confirmación; al confirmar, `onGo`. */
function SinCifrar({
  busy,
  onGo,
  onCancel,
}: {
  busy: boolean;
  onGo: () => void;
  onCancel: () => void;
}) {
  return (
    <fieldset
      aria-label="Confirmar la exportación"
      className="mt-2 space-y-2 rounded-md bg-amber-50 p-3 text-sm text-amber-900"
    >
      <p>
        El ZIP lleva los datos <strong>sin cifrar</strong>. Guárdalo en un disco cifrado o en un
        sitio seguro. Se pide Touch ID y queda en la auditoría con su huella.
      </p>
      <div className="flex gap-2">
        <button type="button" className={PRIMARY} disabled={busy} onClick={onGo}>
          {busy ? 'Preparando…' : 'Entendido, elegir dónde guardarlo'}
        </button>
        <button type="button" className={BUTTON} disabled={busy} onClick={onCancel}>
          Cancelar
        </button>
      </div>
    </fieldset>
  );
}

function ArchivoEjercicio({ years }: { years: number[] }) {
  const [year, setYear] = useState<number | null>(years[years.length - 1] ?? null);
  const [partes, setPartes] = useState<ParteArchivo[] | null>(null);
  const [confirmar, setConfirmar] = useState(false);
  const [busy, setBusy] = useState(false);
  const [hecho, setHecho] = useState<ArchivoHecho | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (year === null) return;
    setPartes(null);
    setHecho(null);
    archivoEjercicioContenido(year)
      .then(setPartes)
      .catch((e) => setError(errorMessage(e)));
  }, [year]);

  async function exportar() {
    if (year === null) return;
    setBusy(true);
    setError(null);
    try {
      const path = await pickZipSave(`Archivo del ejercicio ${year}.zip`);
      if (!path) return;
      setHecho(await archivoEjercicioExportar(year, path));
      setConfirmar(false);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="mt-8" aria-labelledby="archivo-ej-title">
      <div className="flex items-center gap-3">
        <span className="icon-tile" aria-hidden="true">
          <FileArchive size={18} />
        </span>
        <h2 id="archivo-ej-title" className="text-lg font-semibold">
          Archivo del ejercicio
        </h2>
      </div>
      <p className="mt-1 text-sm text-stone-600">
        Un ZIP con lo que hay que conservar del ejercicio en formatos abiertos: diario, mayor y
        sumas y saldos (CSV y PDF), libros registro de IVA, facturas emitidas en PDF y Facturae,
        recibidas con su original, modelos presentados con su justificante, cuentas anuales, actas y
        movimientos bancarios, con un índice y la huella SHA-256 de cada fichero.
      </p>
      {years.length === 0 ? (
        <p className="mt-3 text-sm text-stone-500">Aún no hay ningún ejercicio con datos.</p>
      ) : (
        <>
          <label className="mt-3 block text-sm">
            Ejercicio
            <select
              className="ml-2 rounded-md border border-stone-300 px-2 py-1"
              value={year ?? ''}
              onChange={(e) => setYear(Number(e.target.value))}
            >
              {years.map((y) => (
                <option key={y} value={y}>
                  {y}
                </option>
              ))}
            </select>
          </label>
          {partes && (
            <ul className="mt-3 space-y-1 text-sm" aria-label={`Contenido del archivo ${year}`}>
              {partes.map((p) => (
                <li key={p.clave} className="flex gap-2">
                  <span
                    aria-hidden="true"
                    className={
                      p.estado === 'si'
                        ? 'text-emerald-700'
                        : p.estado === 'parcial'
                          ? 'text-amber-700'
                          : 'text-red-700'
                    }
                  >
                    {MARCA[p.estado]}
                  </span>
                  <span>
                    <strong>{p.titulo}</strong> — {ESTADO[p.estado]}: {p.detalle}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {!confirmar ? (
            <button
              type="button"
              className={`${BUTTON} mt-3`}
              disabled={!partes}
              onClick={() => setConfirmar(true)}
            >
              <Archive size={14} aria-hidden="true" className="mr-1 inline" />
              Generar el archivo de {year}…
            </button>
          ) : (
            <SinCifrar busy={busy} onGo={exportar} onCancel={() => setConfirmar(false)} />
          )}
          {hecho && (
            <p role="status" className="mt-2 break-all text-sm text-emerald-800">
              Guardado {hecho.fichero} ({fileSize(hecho.bytes)}, {hecho.ficheros} ficheros). Huella
              SHA-256: <span className="font-mono">{hecho.sha256}</span> (también en {hecho.fichero}
              .sha256).
            </p>
          )}
        </>
      )}
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-800">
          {error}
        </p>
      )}
    </section>
  );
}

function Plazos({ lista }: { lista: Conservacion[] }) {
  if (lista.length === 0) return null;
  return (
    <section className="mt-8" aria-labelledby="conservacion-title">
      <h2 id="conservacion-title" className="text-lg font-semibold">
        Hasta cuándo conservar cada ejercicio
      </h2>
      <p className="mt-1 text-sm text-stone-600">
        Seis años desde el cierre (art. 30 del Código de Comercio) y, para lo fiscal, mientras no
        prescriba (cuatro años desde el fin del plazo de declarar, arts. 66 y 70 LGT); diez años o
        más si el ejercicio generó bases negativas o deducciones pendientes (art. 66 bis LGT). Un
        requerimiento o una comprobación interrumpen la prescripción: si has tenido alguno, guarda
        ese ejercicio más tiempo. TaxPain no borra nada.
      </p>
      <table className="mt-3 w-full text-left text-sm">
        <thead>
          <tr className="border-b border-stone-200 text-stone-500">
            <th scope="col" className="py-1 pr-2">
              Ejercicio
            </th>
            <th scope="col" className="py-1 pr-2">
              Conservar hasta
            </th>
            <th scope="col" className="py-1 pr-2">
              Por qué
            </th>
            <th scope="col" className="py-1">
              Archivo
            </th>
          </tr>
        </thead>
        <tbody>
          {lista.map((c) => (
            <tr key={c.ejercicio} className="border-b border-stone-100 align-top">
              <td className="py-1 pr-2 font-medium">{c.ejercicio}</td>
              <td className="py-1 pr-2">
                {c.destruible ? 'Ya se puede destruir' : fecha(c.conservar_hasta)}
              </td>
              <td className="py-1 pr-2">{c.motivo}</td>
              <td className="py-1">
                {c.archivado ? (
                  <span title={c.archivado.sha256}>
                    {new Date(c.archivado.creado_en).toLocaleDateString('es-ES')}
                  </span>
                ) : (
                  <span className="text-stone-500">Sin generar</span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function ExportarTodo() {
  const [confirmar, setConfirmar] = useState(false);
  const [busy, setBusy] = useState(false);
  const [hecho, setHecho] = useState<ExportacionCompleta | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function exportar() {
    setBusy(true);
    setError(null);
    try {
      const hoy = new Date().toISOString().slice(0, 10);
      const path = await pickZipSave(`TaxPain-exportacion-completa-${hoy}.zip`);
      if (!path) return;
      setHecho(await exportacionCompleta(path));
      setConfirmar(false);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="mt-8" aria-labelledby="export-title">
      <div className="flex items-center gap-3">
        <span className="icon-tile" aria-hidden="true">
          <Package size={18} />
        </span>
        <h2 id="export-title" className="text-lg font-semibold">
          Exportación completa
        </h2>
      </div>
      <p className="mt-1 text-sm text-stone-600">
        Todos los datos de la empresa en formatos abiertos, para no depender de TaxPain: cada tabla
        en CSV y JSON y los documentos tal cual, con un LEEME que explica qué es cada cosa. Sin las
        claves de los certificados ni de los servicios conectados.
      </p>
      {!confirmar ? (
        <button type="button" className={`${BUTTON} mt-3`} onClick={() => setConfirmar(true)}>
          Exportar todos los datos…
        </button>
      ) : (
        <SinCifrar busy={busy} onGo={exportar} onCancel={() => setConfirmar(false)} />
      )}
      {hecho && (
        <p role="status" className="mt-2 break-all text-sm text-emerald-800">
          Guardado {hecho.fichero} ({fileSize(hecho.bytes)}): {hecho.tablas} tablas, {hecho.filas}{' '}
          filas y {hecho.adjuntos} documentos. Huella SHA-256:{' '}
          <span className="font-mono">{hecho.sha256}</span>
          {hecho.con_error.length > 0 &&
            ` · No se han podido leer enteras: ${hecho.con_error.join(', ')}.`}
        </p>
      )}
      {error && (
        <p role="alert" className="mt-2 text-sm text-red-800">
          {error}
        </p>
      )}
    </section>
  );
}

/** Archivo del ejercicio, plazos de conservación y exportación completa. */
export function Archivo() {
  const [lista, setLista] = useState<Conservacion[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    archivoConservacion()
      .then(setLista)
      .catch((e) => {
        setLista([]);
        setError(errorMessage(e));
      });
  }, []);
  if (lista === null) return null;
  return (
    <>
      {error && (
        <p role="alert" className="mt-4 text-sm text-red-800">
          {error}
        </p>
      )}
      <ArchivoEjercicio years={lista.map((c) => c.ejercicio)} />
      <Plazos lista={lista} />
      <ExportarTodo />
    </>
  );
}

/** Aviso del Panel: qué ejercicios ya se podrían destruir y cuáles no (sin borrar nada). */
export function AvisoArchivo({ onOpen }: { onOpen: () => void }) {
  const [a, setA] = useState<AvisoConservacion | null>(null);
  useEffect(() => {
    archivoAviso()
      .then(setA)
      .catch(() => setA(null));
  }, []);
  if (!a || (a.destruibles.length === 0 && a.sin_archivo.length === 0)) return null;
  const vivos = a.conservar.filter((c) => c.cierre < new Date().toISOString().slice(0, 10));
  return (
    <section
      aria-labelledby="aviso-archivo"
      className="mt-4 rounded-md border border-stone-200 bg-white p-3 text-sm"
    >
      <h3 id="aviso-archivo" className="font-semibold">
        Archivo y conservación
      </h3>
      {a.destruibles.length > 0 && (
        <p className="mt-1">
          Ya se podrían destruir (han pasado los plazos legales): {a.destruibles.join(', ')}.
          TaxPain no borra nada: decide tú.
        </p>
      )}
      {vivos.length > 0 && (
        <p className="mt-1">
          Hay que conservar:{' '}
          {vivos.map((c) => `${c.ejercicio} (hasta ${fecha(c.conservar_hasta)})`).join(', ')}.
        </p>
      )}
      {a.sin_archivo.length > 0 && (
        <p className="mt-1">Sin «Archivo del ejercicio» generado: {a.sin_archivo.join(', ')}.</p>
      )}
      <button type="button" className="mt-2 underline" onClick={onOpen}>
        Ir a Archivo y auditoría
      </button>
    </section>
  );
}
