import { useCallback, useEffect, useState } from 'react';
import {
  assetDelete,
  assetEdit,
  assetIncentiveUsage,
  assetSetIncentive,
  assetUndispose,
  type DepreciationDifference,
  depreciationDifferences,
  depreciationPreview,
  depreciationUndo,
  euros,
  type FixedAsset,
  formatDate,
  type Incentivo,
  type PrevisionAmortizacion,
  parseEuros,
  plural,
  postDepreciation,
  type UsoIncentivos,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/** «25» o «12,5» → puntos básicos; vacío → null; inválido → undefined. */
function percentBp(s: string): number | null | undefined {
  if (!s.trim()) return null;
  const n = Number(s.replace(',', '.'));
  return n > 0 && n <= 100 ? Math.round(n * 100) : undefined;
}

const pct = (bp: number) => (bp / 100).toLocaleString('es-ES');
const plainEuros = (c: number) => (c / 100).toFixed(2).replace('.', ',');

/** Vida útil en años de un coeficiente lineal (100 / coeficiente, hacia arriba). */
export function usefulLife(bp: number): number {
  return Math.ceil(10_000 / Math.max(bp, 1));
}

/** Nombre corto del incentivo, para la ficha del elemento. */
export function incentiveLabel(i: Incentivo): string {
  switch (i.tipo) {
    case 'escaso_valor':
      return 'escaso valor (art. 12.3.e LIS)';
    case 'erd_empleo':
      return `libertad con creación de empleo (art. 102 LIS, +${(i.incremento_plantilla / 100).toLocaleString('es-ES')} personas)`;
    case 'erd_acelerada':
      return `amortización acelerada (art. 103 LIS, tablas ${pct(i.coef_tablas)} %)`;
    default:
      return 'libertad de amortización';
  }
}

/**
 * Editar los datos de un elemento (nombre, puesta en funcionamiento, coeficiente y valor
 * residual) y borrarlo si se dio de alta por error.
 */
export function EditarBien({
  asset,
  onDone,
  onMsg,
}: {
  asset: FixedAsset;
  onDone: () => void;
  onMsg: (m: string) => void;
}) {
  const [form, setForm] = useState({
    name: asset.name,
    date: asset.acquired_on,
    rate: String(asset.rate / 100).replace('.', ','),
    residual: plainEuros(asset.residual),
  });
  const rate = percentBp(form.rate);
  const residual = parseEuros(form.residual || '0');
  const valid = form.name.trim() !== '' && form.date !== '' && rate != null && residual !== null;
  return (
    <div className="mt-2 flex flex-wrap items-end gap-3 rounded-md bg-stone-50 p-3">
      <label>
        Nombre
        <input
          className="mt-1 block"
          value={form.name}
          onChange={(e) => setForm({ ...form, name: e.target.value })}
        />
      </label>
      <label>
        Puesta en funcionamiento
        <input
          type="date"
          className="mt-1 block"
          value={form.date}
          onChange={(e) => setForm({ ...form, date: e.target.value })}
        />
      </label>
      <label>
        Coeficiente % anual
        <input
          className="mt-1 block w-24"
          inputMode="decimal"
          value={form.rate}
          onChange={(e) => setForm({ ...form, rate: e.target.value })}
        />
      </label>
      <label>
        Valor residual
        <input
          className="mt-1 block w-32 text-right"
          inputMode="decimal"
          value={form.residual}
          onChange={(e) => setForm({ ...form, residual: e.target.value })}
        />
      </label>
      {rate != null && <span className="text-stone-600">Vida útil: {usefulLife(rate)} años</span>}
      <ConfirmAction
        label="Guardar los cambios"
        className="rounded btn-primary px-3 py-1 disabled:opacity-50"
        disabled={!valid}
        danger={false}
        question={`Se guardan los datos nuevos de ${asset.name}. Las amortizaciones ya registradas no cambian; las de los ejercicios sin registrar se calculan con estos datos. ¿Seguir?`}
        confirmLabel="Sí, guardar"
        onConfirm={async () => {
          if (!valid || rate == null || residual === null) return;
          const avisos = await assetEdit(asset.id, {
            name: form.name.trim(),
            acquired_on: form.date,
            rate,
            residual,
          });
          onMsg([`«${form.name.trim()}» actualizado.`, ...avisos].join(' '));
          onDone();
        }}
      />
      <ConfirmAction
        label="Borrar el elemento"
        className="text-red-700 underline"
        question={`Se borra ${asset.name} del inmovilizado. Sólo se puede si no tiene asientos (amortizaciones o baja): es para un alta hecha por error. ¿Seguir?`}
        confirmLabel="Sí, borrar"
        onConfirm={async () => {
          await assetDelete(asset.id);
          onMsg(`«${asset.name}» borrado del inmovilizado.`);
          onDone();
        }}
      />
    </div>
  );
}

/** Deshacer la baja o la venta de un elemento. */
export function DeshacerBaja({
  asset,
  onDone,
  onMsg,
}: {
  asset: FixedAsset;
  onDone: () => void;
  onMsg: (m: string) => void;
}) {
  if (!asset.disposed_on) return null;
  return (
    <ConfirmAction
      label="Deshacer la baja"
      className="ml-3 text-stone-700 underline"
      question={`Se borra el asiento de la baja del ${formatDate(asset.disposed_on)} de ${asset.name} y su amortización vuelve a como estaba antes (la del ejercicio, hasta el cierre; la de los ejercicios siguientes, si la baja la quitó). Si tiene la factura de venta vinculada, desvincúlala antes. ¿Seguir?`}
      confirmLabel="Sí, deshacer"
      onConfirm={async () => {
        const r = await assetUndispose(asset.id);
        onMsg([`Baja de ${asset.name} deshecha.`, ...r.warnings].join(' '));
        onDone();
      }}
    />
  );
}

type TipoIncentivo = Incentivo['tipo'] | '';

/** Incentivo fiscal de la amortización de un elemento, con sus topes. */
export function IncentivoFiscal({
  asset,
  incentivo,
  onDone,
  onMsg,
}: {
  asset: FixedAsset;
  incentivo: Incentivo | null;
  onDone: () => void;
  onMsg: (m: string) => void;
}) {
  const [tipo, setTipo] = useState<TipoIncentivo>(incentivo?.tipo ?? '');
  const [param, setParam] = useState(
    incentivo?.tipo === 'erd_empleo'
      ? String(incentivo.incremento_plantilla / 100).replace('.', ',')
      : incentivo?.tipo === 'erd_acelerada'
        ? String(incentivo.coef_tablas / 100).replace('.', ',')
        : '',
  );
  const [taxRate, setTaxRate] = useState(
    asset.tax_rate == null ? '' : String(asset.tax_rate / 100).replace('.', ','),
  );
  const build = (): Incentivo | null | undefined => {
    const n = Number(param.replace(',', '.'));
    switch (tipo) {
      case '':
        return null;
      case 'escaso_valor':
        return { tipo };
      case 'otro':
        return { tipo };
      case 'erd_empleo':
        return n > 0 ? { tipo, incremento_plantilla: Math.round(n * 100) } : undefined;
      case 'erd_acelerada':
        return n > 0 && n <= 100 ? { tipo, coef_tablas: Math.round(n * 100) } : undefined;
    }
  };
  const conCoeficiente = tipo === 'erd_acelerada' || tipo === 'otro' || tipo === '';
  return (
    <div className="mt-2 flex flex-wrap items-end gap-3 rounded-md bg-stone-50 p-3">
      <label>
        Incentivo fiscal
        <select
          className="mt-1 block"
          value={tipo}
          onChange={(e) => setTipo(e.target.value as TipoIncentivo)}
        >
          <option value="">Ninguno</option>
          <option value="escaso_valor">Escaso valor: hasta 300 €, 25.000 € al año (12.3.e)</option>
          <option value="erd_empleo">Libertad con creación de empleo, ERD (art. 102)</option>
          <option value="erd_acelerada">Amortización acelerada, ERD (art. 103)</option>
          <option value="otro">Otra libertad de amortización</option>
        </select>
      </label>
      {tipo === 'erd_empleo' && (
        <label>
          Incremento de plantilla media (personas, con dos decimales)
          <input
            className="mt-1 block w-24"
            inputMode="decimal"
            value={param}
            onChange={(e) => setParam(e.target.value)}
          />
        </label>
      )}
      {tipo === 'erd_acelerada' && (
        <label>
          Coeficiente máximo de tablas %
          <input
            className="mt-1 block w-24"
            inputMode="decimal"
            value={param}
            onChange={(e) => setParam(e.target.value)}
          />
        </label>
      )}
      {conCoeficiente && (
        <label>
          Coeficiente fiscal %{tipo === 'erd_acelerada' ? ' (vacío: el doble de tablas)' : ''}
          <input
            className="mt-1 block w-24"
            inputMode="decimal"
            value={taxRate}
            onChange={(e) => setTaxRate(e.target.value)}
          />
        </label>
      )}
      <ConfirmAction
        label="Guardar el incentivo"
        className="rounded btn-primary px-3 py-1"
        danger={false}
        question={`Se cambia el plan fiscal de ${asset.name}: la diferencia con la amortización contable va como ajuste al modelo 200 y se revierte en los años siguientes y en la baja. ¿Seguir?`}
        confirmLabel="Sí, guardar"
        onConfirm={async () => {
          const i = build();
          const t = conCoeficiente ? percentBp(taxRate) : null;
          if (i === undefined || t === undefined) {
            onMsg('Revisa los datos del incentivo');
            return;
          }
          const avisos = await assetSetIncentive(asset.id, i, t ?? null);
          onMsg(
            [
              i
                ? `Incentivo de ${asset.name}: ${incentiveLabel(i)}.`
                : `${asset.name} sin incentivo.`,
              ...avisos,
            ].join(' '),
          );
          onDone();
        }}
      />
      <p className="basis-full text-xs text-stone-600">
        Los topes (300 € por elemento y 25.000 € al año; 120.000 € por persona de aumento de
        plantilla; el doble del coeficiente de tablas) se comprueban aquí. Ser empresa de reducida
        dimensión, que el elemento sea nuevo y mantener el empleo, no: compruébalo tú.
      </p>
    </div>
  );
}

/**
 * Registrar (con su total a la vista) y deshacer las amortizaciones del ejercicio, el uso de los
 * topes de los incentivos y las diferencias que van al 200.
 */
export function AmortizacionesEjercicio({
  year,
  version,
  onDone,
  onMsg,
}: {
  year: number;
  version: number;
  onDone: () => void;
  onMsg: (m: string) => void;
}) {
  const [prev, setPrev] = useState<PrevisionAmortizacion | null>(null);
  const [uso, setUso] = useState<UsoIncentivos | null>(null);
  const [difs, setDifs] = useState<DepreciationDifference[]>([]);
  const load = useCallback(() => {
    depreciationPreview(year)
      .then(setPrev)
      .catch(() => setPrev(null));
    assetIncentiveUsage(year)
      .then(setUso)
      .catch(() => setUso(null));
    depreciationDifferences(year)
      .then(setDifs)
      .catch(() => setDifs([]));
  }, [year]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: recarga cuando cambian los elementos
  useEffect(load, [load, version]);
  const done = () => {
    load();
    onDone();
  };
  const pendientes = prev?.pendientes ?? [];
  const registradas = prev?.registradas ?? [];
  const difs200 = difs.filter((d) => d.difference !== 0);
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-4">
        <ConfirmAction
          label={`Registrar amortizaciones de ${year}`}
          className="text-link underline disabled:opacity-50"
          disabled={!prev || pendientes.length === 0}
          danger={false}
          question={
            <>
              Se registran {plural(pendientes.length, 'asiento', 'asientos')} de amortización de{' '}
              {year} por {euros(prev?.total ?? 0)} en total:
              <ul className="ml-4 list-disc">
                {pendientes.map((l) => (
                  <li key={l.asset_id}>
                    {l.name}: {euros(l.amount)}
                  </li>
                ))}
              </ul>
              ¿Seguir?
            </>
          }
          confirmLabel="Sí, registrar"
          onConfirm={async () => {
            const n = await postDepreciation(year);
            onMsg(
              `${plural(n, 'asiento', 'asientos')} de amortización de ${year} ${n === 1 ? 'registrado' : 'registrados'}.`,
            );
            done();
          }}
        />
        {prev && pendientes.length === 0 && (
          <span className="text-stone-600">
            {registradas.length > 0
              ? `Las amortizaciones de ${year} ya están registradas (${euros(prev.total_registrado)}).`
              : `No hay amortizaciones de ${year} que registrar.`}
          </span>
        )}
        {prev && registradas.length > 0 && !prev.cerrado && (
          <ConfirmAction
            label={`Deshacer la amortización de ${year}`}
            className="text-red-700 underline"
            question={`Se borran los ${plural(registradas.length, 'asiento', 'asientos')} de amortización de ${year} (${euros(prev.total_registrado)}). No se deshace la de un elemento dado de baja (es parte de la baja) ni la que no es la última registrada del elemento. ¿Seguir?`}
            confirmLabel="Sí, deshacer"
            onConfirm={async () => {
              const r = await depreciationUndo(year);
              onMsg(
                [
                  r.borrados > 0
                    ? `${plural(r.borrados, 'asiento', 'asientos')} de amortización de ${year} ${r.borrados === 1 ? 'borrado' : 'borrados'} (${euros(r.total)}).`
                    : `No se ha deshecho ninguna amortización de ${year}.`,
                  ...r.saltados,
                ].join(' '),
              );
              done();
            }}
          />
        )}
      </div>
      {uso && (uso.escaso_valor > 0 || uso.erd_empleo > 0) && (
        <p className="text-stone-600">
          Incentivos de {year}:
          {uso.escaso_valor > 0 &&
            ` escaso valor ${euros(uso.escaso_valor)} de ${euros(uso.escaso_valor_tope)}`}
          {uso.erd_empleo > 0 &&
            ` · libertad con creación de empleo ${euros(uso.erd_empleo)}${uso.erd_empleo_tope != null ? ` de ${euros(uso.erd_empleo_tope)}` : ''}`}
          .
        </p>
      )}
      {difs200.length > 0 && (
        <table className="text-sm">
          <caption className="text-left font-semibold">
            Diferencias de amortización de {year} para el modelo 200
          </caption>
          <thead>
            <tr>
              <th className="pr-3 text-left">Elemento</th>
              <th className="pr-3 text-right">Contable</th>
              <th className="pr-3 text-right">Fiscal</th>
              <th className="text-right">Ajuste</th>
            </tr>
          </thead>
          <tbody>
            {difs200.map((d) => (
              <tr key={d.asset_id}>
                <td className="pr-3">
                  {d.name}
                  {d.reversal && ' (baja: se revierte lo acumulado)'}
                </td>
                <td className="pr-3 text-right">{euros(d.accounting)}</td>
                <td className="pr-3 text-right">{euros(d.tax)}</td>
                <td className="text-right">
                  {d.difference > 0
                    ? `+${euros(d.difference)} aumento`
                    : `${euros(-d.difference)} disminución`}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
