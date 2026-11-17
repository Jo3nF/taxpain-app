import { useCallback, useEffect, useState } from 'react';
import {
  type BienExtranjeroView,
  type BloqueExtranjero,
  type BorradorExtranjero,
  type CondicionDeclarante,
  type DatosBienExtranjero,
  errorMessage,
  euros,
  extranjeroBorrador,
  extranjeroExportar,
  extranjeroGuardarBien,
  extranjeroGuardarDeclarado,
  extranjeroMarcarPresentado,
  extranjeroQuitarDeclarado,
  formatDate,
  parseEuros,
  pickFolder,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const CONDICIONES: [CondicionDeclarante, string][] = [
  ['titular', 'Titular'],
  ['representante', 'Representante'],
  ['autorizado', 'Autorizado'],
  ['beneficiario', 'Beneficiario'],
  ['usufructuario', 'Usufructuario'],
  ['tomador', 'Tomador'],
  ['poder_disposicion', 'Con poder de disposición'],
  ['titular_real', 'Otras formas de titularidad real'],
  ['otra', 'Otra'],
];

const ALTA: Record<string, string> = {
  A: 'Alta (primera vez)',
  M: 'Ya declarado',
  C: 'Extinguido',
};

const ORIGEN: Record<DatosBienExtranjero['origen'], string> = {
  banco: 'Cuenta bancaria',
  valores: 'Cuenta de valores',
  cripto: 'Plataforma de criptomonedas',
};

function pct(bp: number): string {
  return `${(bp / 100).toLocaleString('es-ES', { maximumFractionDigits: 2 })} %`;
}

/** Borrador del 720 (cuentas y valores en el extranjero) o del 721 (monedas virtuales). */
export function BienesExtranjero({ model, year }: { model: '720' | '721'; year: number }) {
  const [b, setB] = useState<BorradorExtranjero | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    extranjeroBorrador(model, year)
      .then(setB)
      .catch((e) => {
        setB(null);
        setError(errorMessage(e));
      });
  }, [model, year]);
  useEffect(load, [load]);

  const titulo =
    model === '720'
      ? `Modelo 720 de ${year}: bienes y derechos en el extranjero`
      : `Modelo 721 de ${year}: monedas virtuales en el extranjero`;

  return (
    <section aria-labelledby="extranjero-h" className="mt-6">
      <h3 id="extranjero-h" className="text-base font-semibold">
        {titulo}
      </h3>
      {error && (
        <p role="alert" className="mt-3 text-sm text-red-700">
          {error}
        </p>
      )}
      {b && (
        <>
          <p className="mt-2 text-sm">
            {b.exenta_por_contabilidad
              ? `Persona jurídica: no presenta el ${model} de los bienes contabilizados de forma individualizada${b.requisitos.length ? ', pero falta algún requisito (abajo).' : '.'}`
              : b.obligado
                ? `Hay que presentar el ${model}: del 1 de enero al ${formatDate(b.plazo)}.`
                : `Con estos datos no hay que presentar el ${model} de ${year}.`}
          </p>
          <p role="note" className="mt-2 rounded border border-amber-300 bg-amber-50 p-2 text-sm">
            {b.fichero.nota}
          </p>
          <div className="form-actions mt-3 text-sm">
            <button
              type="button"
              className="rounded btn-primary px-3 py-1.5"
              onClick={async () => {
                const dir = await pickFolder();
                if (!dir) return;
                try {
                  setMsg(`Hoja guardada en ${await extranjeroExportar(model, year, dir)}`);
                } catch (e) {
                  setMsg(errorMessage(e));
                }
              }}
            >
              Guardar hoja de registros (Excel)…
            </button>
            {b.obligado && (
              <ConfirmAction
                label="Ya lo he presentado"
                question={`¿Apuntar el ${model} de ${year} como presentado con estos importes? Sirve para saber si hay que volver a presentarlo los años siguientes.`}
                confirmLabel="Apuntar"
                danger={false}
                className="btn-ghost"
                onConfirm={async () => {
                  await extranjeroMarcarPresentado(model, year, true);
                  setMsg(`${model} de ${year} apuntado como presentado.`);
                  load();
                }}
              />
            )}
          </div>
          {msg && (
            <p role="status" className="mt-2 text-sm">
              {msg}
            </p>
          )}
          {b.bloques.map((bl) => (
            <section
              key={bl.bloque}
              aria-labelledby={`ext-${bl.bloque}`}
              className="mt-4 rounded-md border border-stone-200 p-3"
            >
              <h4 id={`ext-${bl.bloque}`} className="font-medium">
                {bl.titulo}: {bl.obligado ? 'se presenta' : 'no se presenta'}
              </h4>
              <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                <dt>Valor a {formatDate(b.fecha)}</dt>
                <dd>{euros(bl.total)}</dd>
                {bl.total_medio !== null && (
                  <>
                    <dt>Saldo medio del 4.º trimestre</dt>
                    <dd>{euros(bl.total_medio)}</dd>
                  </>
                )}
                {bl.anterior && (
                  <>
                    <dt>Declarado en {bl.anterior.year}</dt>
                    <dd>
                      {euros(bl.anterior.valor)}
                      {bl.incremento !== null && ` (incremento ${euros(bl.incremento)})`}
                    </dd>
                  </>
                )}
              </dl>
              <p className="mt-2 text-sm text-stone-700">{bl.motivo}</p>
              {bl.registros.length > 0 && (
                <div className="mt-2 overflow-x-auto">
                  <table className="w-full text-sm">
                    <caption className="sr-only">Registros: {bl.titulo}</caption>
                    <thead>
                      <tr className="text-left">
                        <th scope="col">Clave</th>
                        <th scope="col">Origen</th>
                        <th scope="col">Bien</th>
                        <th scope="col">Identificación</th>
                        <th scope="col">Entidad</th>
                        <th scope="col">País</th>
                        <th scope="col">Condición</th>
                        <th scope="col">Apertura o adquisición</th>
                        <th scope="col" className="text-right">
                          Saldo o valor
                        </th>
                        {bl.bloque === 'cuentas' && (
                          <th scope="col" className="text-right">
                            Saldo medio 4T
                          </th>
                        )}
                        <th scope="col">Falta</th>
                      </tr>
                    </thead>
                    <tbody>
                      {bl.registros.map((r) => (
                        <tr
                          key={`${r.origen}-${r.ref_id}-${r.identificacion}`}
                          className="border-t border-stone-100"
                        >
                          <td>{r.clave}</td>
                          <td>{ALTA[r.alta]}</td>
                          <td>
                            {r.descripcion}
                            {r.unidades && ` · ${r.unidades}`}
                          </td>
                          <td>{r.identificacion}</td>
                          <td>{r.entidad}</td>
                          <td>{r.pais}</td>
                          <td>
                            {CONDICIONES.find(([c]) => c === r.condicion)?.[1]}
                            {r.porcentaje_bp !== 10000 && ` (${pct(r.porcentaje_bp)})`}
                          </td>
                          <td>
                            {r.fecha_apertura ? formatDate(r.fecha_apertura) : '—'}
                            {r.fecha_estimada && r.fecha_apertura && ' (primer movimiento)'}
                          </td>
                          <td className="text-right">{euros(r.saldo)}</td>
                          {bl.bloque === 'cuentas' && (
                            <td className="text-right">
                              {r.saldo_medio !== null ? euros(r.saldo_medio) : ''}
                            </td>
                          )}
                          <td>{r.faltan.join(', ')}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </section>
          ))}
          {b.requisitos.length > 0 && (
            <section aria-labelledby="ext-req" className="mt-4">
              <h4 id="ext-req" className="font-medium">
                Requisitos de la exoneración que faltan
              </h4>
              <ul className="mt-1 list-disc pl-5 text-sm">
                {b.requisitos.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </section>
          )}
          {b.avisos.length > 0 && (
            <ul className="mt-4 list-disc pl-5 text-sm text-amber-800">
              {b.avisos.map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
          <ul className="mt-4 list-disc pl-5 text-sm text-stone-600">
            {b.notas.map((n) => (
              <li key={n}>{n}</li>
            ))}
            <li>Norma: {b.norma}.</li>
          </ul>
          <Bienes bienes={b.bienes} onSaved={load} />
          <Declarados b={b} onChange={load} />
        </>
      )}
    </section>
  );
}

function Bienes({ bienes, onSaved }: { bienes: BienExtranjeroView[]; onSaved: () => void }) {
  if (bienes.length === 0) {
    return (
      <p className="mt-4 text-sm text-stone-600">
        No hay cuentas, brókeres ni plataformas que datar todavía.
      </p>
    );
  }
  return (
    <section aria-labelledby="ext-bienes" className="mt-6">
      <h4 id="ext-bienes" className="font-medium">
        Datos de los bienes
      </h4>
      <p className="mt-1 text-sm text-stone-600">
        Lo que la contabilidad no sabe: tu condición, la fecha de apertura, la entidad con su NIF,
        BIC y domicilio, y el país.
      </p>
      <ul className="mt-2 space-y-2">
        {bienes.map((x) => (
          <li key={`${x.datos.origen}-${x.datos.ref_id}`}>
            <BienForm bien={x} onSaved={onSaved} />
          </li>
        ))}
      </ul>
    </section>
  );
}

function BienForm({ bien, onSaved }: { bien: BienExtranjeroView; onSaved: () => void }) {
  const [d, setD] = useState<DatosBienExtranjero>(bien.datos);
  const [porcentaje, setPorcentaje] = useState(String(bien.datos.porcentaje_bp / 100));
  const [estado, setEstado] = useState<string | null>(null);
  const texto = (k: keyof DatosBienExtranjero, label: string, max = 120) => (
    <label className="block">
      {label}
      <input
        className="mt-1 block w-full"
        maxLength={max}
        value={String(d[k] ?? '')}
        onChange={(e) => setD({ ...d, [k]: e.target.value })}
      />
    </label>
  );
  return (
    <details className="rounded border border-stone-200 p-2 text-sm">
      <summary>
        {bien.nombre} · {ORIGEN[d.origen]}
        {d.origen === 'banco' && (bien.extranjero ? ' · del extranjero' : ' · en España')}
      </summary>
      <form
        className="mt-2 grid gap-2 sm:grid-cols-2"
        onSubmit={async (e) => {
          e.preventDefault();
          const p = Number(porcentaje.replace(',', '.'));
          if (!Number.isFinite(p) || p <= 0 || p > 100) {
            setEstado('El porcentaje va de 0,01 a 100.');
            return;
          }
          try {
            await extranjeroGuardarBien({ ...d, porcentaje_bp: Math.round(p * 100) });
            setEstado('Guardado.');
            onSaved();
          } catch (err) {
            setEstado(errorMessage(err));
          }
        }}
      >
        {d.origen === 'banco' && (
          <label className="flex items-center gap-2 sm:col-span-2">
            <input
              type="checkbox"
              checked={d.extranjero ?? bien.extranjero}
              onChange={(e) => setD({ ...d, extranjero: e.target.checked })}
            />
            Cuenta en una entidad del extranjero
          </label>
        )}
        <label className="block">
          Condición del declarante
          <select
            className="mt-1 block w-full"
            value={d.condicion}
            onChange={(e) => setD({ ...d, condicion: e.target.value as CondicionDeclarante })}
          >
            {CONDICIONES.map(([c, l]) => (
              <option key={c} value={c}>
                {l}
              </option>
            ))}
          </select>
        </label>
        <label className="block">
          Participación (%)
          <input
            className="mt-1 block w-full"
            inputMode="decimal"
            value={porcentaje}
            onChange={(e) => setPorcentaje(e.target.value)}
          />
        </label>
        <label className="block">
          {d.origen === 'banco' ? 'Fecha de apertura' : 'Fecha de alta en la entidad'}
          <input
            type="date"
            className="mt-1 block w-full"
            value={d.fecha_apertura ?? ''}
            onChange={(e) => setD({ ...d, fecha_apertura: e.target.value || null })}
          />
        </label>
        <label className="block">
          Fecha de cancelación o extinción
          <input
            type="date"
            className="mt-1 block w-full"
            value={d.fecha_extincion ?? ''}
            onChange={(e) => setD({ ...d, fecha_extincion: e.target.value || null })}
          />
        </label>
        {texto('entidad', d.origen === 'cripto' ? 'Custodio' : 'Entidad')}
        {texto('nif_entidad', 'NIF de la entidad en su país', 40)}
        {d.origen === 'banco' && texto('bic', 'BIC', 11)}
        {texto('domicilio', 'Domicilio de la entidad', 200)}
        {texto('pais', 'País (dos letras; vacío: el del IBAN o la cuenta)', 2)}
        <div className="form-actions sm:col-span-2">
          <button type="submit" className="rounded border border-stone-300 px-3 py-1">
            Guardar datos
          </button>
          {estado && <span role="status">{estado}</span>}
        </div>
      </form>
    </details>
  );
}

function Declarados({ b, onChange }: { b: BorradorExtranjero; onChange: () => void }) {
  const bloques: BloqueExtranjero[] = b.modelo === '720' ? ['cuentas', 'valores'] : ['monedas'];
  const [year, setYear] = useState(String(b.year - 1));
  const [bloque, setBloque] = useState<BloqueExtranjero>(bloques[0] ?? 'monedas');
  const [valor, setValor] = useState('');
  const [medio, setMedio] = useState('');
  const [bienes, setBienes] = useState('');
  const [estado, setEstado] = useState<string | null>(null);
  const nombre = (x: BloqueExtranjero) =>
    x === 'cuentas' ? 'Cuentas' : x === 'valores' ? 'Valores' : 'Monedas virtuales';
  return (
    <section aria-labelledby="ext-decl" className="mt-6">
      <h4 id="ext-decl" className="font-medium">
        Lo declarado otros años
      </h4>
      <p className="mt-1 text-sm text-stone-600">
        Con el valor de cada bloque del último {b.modelo} presentado se decide si hay que volver a
        presentarlo (incremento de más de 20.000 € o algún bien extinguido).
        {b.presentados_aeat.length > 0 &&
          ` Según la AEAT o lo importado, consta presentado en ${b.presentados_aeat.join(', ')}.`}
      </p>
      {b.declarados.length > 0 && (
        <ul className="mt-2 divide-y divide-stone-200 text-sm">
          {b.declarados.map((d) => (
            <li key={d.id} className="flex flex-wrap items-center gap-3 py-2">
              <span className="flex-1">
                {d.year} · {nombre(d.bloque)} · {euros(d.valor)}
                {d.saldo_medio !== null && ` (saldo medio ${euros(d.saldo_medio)})`}
                {d.bienes.length > 0 && ` · ${d.bienes.join(', ')}`}
                {d.origen === 'borrador' ? ' · desde el borrador' : ' · tecleado'}
              </span>
              <ConfirmAction
                label="Quitar"
                ariaLabel={`Quitar lo declarado: ${nombre(d.bloque)} de ${d.year}`}
                question={`¿Quitar lo declarado de ${nombre(d.bloque).toLowerCase()} en ${d.year}? Dejará de contar para la regla de los 20.000 €.`}
                confirmLabel="Quitar"
                onConfirm={async () => {
                  await extranjeroQuitarDeclarado(d.id, true);
                  onChange();
                }}
              />
            </li>
          ))}
        </ul>
      )}
      <form
        className="mt-3 flex flex-wrap items-end gap-3 text-sm"
        onSubmit={async (e) => {
          e.preventDefault();
          const v = parseEuros(valor);
          const m = medio.trim() ? parseEuros(medio) : null;
          if (v === null || (medio.trim() && m === null)) {
            setEstado('Importe no válido.');
            return;
          }
          try {
            await extranjeroGuardarDeclarado({
              id: 0,
              modelo: b.modelo,
              year: Number(year),
              bloque,
              valor: v,
              saldo_medio: bloque === 'cuentas' ? m : null,
              bienes: bienes
                .split(/[,;\n]/)
                .map((x) => x.trim())
                .filter(Boolean),
              origen: 'tecleado',
              creado: '',
            });
            setEstado('Guardado.');
            setValor('');
            setMedio('');
            setBienes('');
            onChange();
          } catch (err) {
            setEstado(errorMessage(err));
          }
        }}
      >
        <label>
          Ejercicio
          <input
            className="mt-1 block w-24"
            inputMode="numeric"
            value={year}
            onChange={(e) => setYear(e.target.value)}
          />
        </label>
        {bloques.length > 1 && (
          <label>
            Bloque
            <select
              className="mt-1 block"
              value={bloque}
              onChange={(e) => setBloque(e.target.value as BloqueExtranjero)}
            >
              {bloques.map((x) => (
                <option key={x} value={x}>
                  {nombre(x)}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          Valor declarado
          <input
            className="mt-1 block w-32"
            inputMode="decimal"
            value={valor}
            onChange={(e) => setValor(e.target.value)}
          />
        </label>
        {bloque === 'cuentas' && (
          <label>
            Saldo medio 4T declarado
            <input
              className="mt-1 block w-32"
              inputMode="decimal"
              value={medio}
              onChange={(e) => setMedio(e.target.value)}
            />
          </label>
        )}
        <label className="min-w-64 flex-1">
          {bloque === 'monedas'
            ? 'Monedas declaradas (BTC@Plataforma, separadas por comas)'
            : 'IBAN o ISIN declarados (separados por comas)'}
          <input
            className="mt-1 block w-full"
            value={bienes}
            onChange={(e) => setBienes(e.target.value)}
          />
        </label>
        <button type="submit" className="rounded border border-stone-300 px-3 py-1">
          Añadir lo declarado
        </button>
        {estado && <span role="status">{estado}</span>}
      </form>
    </section>
  );
}
