// Ventanilla única (OSS) en la factura a un particular de otro país de la UE: el IVA del país del
// cliente (servicios electrónicos o de telecomunicaciones y ventas a distancia de bienes) cuando
// se supera el umbral de 10.000 € o se opta por ello (art. 73 de la Ley del IVA).

import { useEffect, useState } from 'react';
import {
  type EstadoOss,
  errorMessage,
  type OssSupply,
  ossConfirmarTipo,
  ossEstado,
  ossTipos,
  type TiposOss,
} from './api';
import { ConfirmAction } from './ConfirmAction';

/** Lo que guarda el formulario de la factura (el tipo, tal como se escribe). */
export interface FormOss {
  rate: string;
  supply: OssSupply;
}

/** Centésimas de punto → «20 %», «25,5 %». */
export function pctText(bp: number): string {
  const s = (bp / 100).toFixed(2).replace(/\.?0+$/, '');
  return `${s.replace('.', ',')} %`;
}

/** «20», «25,5» o «25,5 %» → centésimas de punto (entre 0,01 % y 35 %), o null. */
export function parsePct(s: string): number | null {
  const t = s.replace('%', '').trim().replace(',', '.');
  if (!/^\d{1,2}(\.\d{1,2})?$/.test(t)) return null;
  const bp = Math.round(Number(t) * 100);
  return bp >= 1 && bp <= 3500 ? bp : null;
}

export const SUPPLY_LABEL: Record<OssSupply, string> = {
  Servicios: 'Servicio electrónico, de telecomunicaciones o de radiodifusión',
  Bienes: 'Venta a distancia de bienes',
};

const input = 'mt-1 block w-full rounded-md border border-stone-300 px-2 py-1';

export function OssFields({
  country,
  year,
  oss,
  onChange,
}: {
  country: string;
  year: number;
  oss: FormOss | undefined;
  onChange: (oss: FormOss | undefined) => void;
}) {
  const [tipos, setTipos] = useState<TiposOss | null>(null);
  const [estado, setEstado] = useState<EstadoOss | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    ossTipos()
      .then(setTipos)
      .catch((e) => setError(errorMessage(e)));
  };
  useEffect(load, []);
  useEffect(() => {
    ossEstado(year)
      .then(setEstado)
      .catch(() => setEstado(null));
  }, [year]);

  const code = country === 'GR' ? 'EL' : country;
  const tipo = tipos?.paises.find((p) => p.pais === code);
  const sugerido = tipo ? (tipo.confirmado ?? tipo.tabla) : null;
  const bp = oss ? parsePct(oss.rate) : null;

  return (
    <div className="rounded-md border border-sky-200 bg-sky-50 p-3 text-sm">
      {estado?.tributa_en_destino && !oss && (
        <p role="alert" className="mb-2 text-amber-800">
          Ya tributas en el país del cliente desde el {estado.destino_desde}: si es un servicio
          electrónico o de telecomunicaciones o una venta a distancia de bienes, marca la casilla de
          abajo (ventanilla única).
        </p>
      )}
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={oss !== undefined}
          onChange={(e) =>
            onChange(
              e.target.checked
                ? {
                    rate: sugerido !== null ? pctText(sugerido).replace(' %', '') : '',
                    supply: 'Servicios',
                  }
                : undefined,
            )
          }
        />
        Lleva el IVA del país del cliente (ventanilla única, OSS: se declara en el modelo 369)
      </label>
      {oss && (
        <div className="mt-2 grid grid-cols-2 gap-3">
          <label>
            Operación
            <select
              className={input}
              value={oss.supply}
              onChange={(e) => onChange({ ...oss, supply: e.target.value as OssSupply })}
            >
              {(['Servicios', 'Bienes'] as const).map((s) => (
                <option key={s} value={s}>
                  {SUPPLY_LABEL[s]}
                </option>
              ))}
            </select>
          </label>
          <label>
            Tipo de IVA de {tipo?.nombre ?? country} (%)
            <input
              className={input}
              inputMode="decimal"
              value={oss.rate}
              onChange={(e) => onChange({ ...oss, rate: e.target.value })}
            />
          </label>
          <div className="col-span-2 text-xs text-stone-600">
            {tipo?.confirmado != null ? (
              <span>
                Tipo general confirmado: {pctText(tipo.confirmado)}. Pon otro si la operación va a
                un tipo reducido en {tipo.nombre}.
              </span>
            ) : tipo ? (
              <span className="flex flex-wrap items-center gap-2">
                Tipo general de la tabla de TaxPain (a {tipos?.fecha}): {pctText(tipo.tabla)}, sin
                confirmar. Compruébalo en la fuente oficial antes de emitir.
                <ConfirmAction
                  label={`Confirmar el ${pctText(bp ?? tipo.tabla)}`}
                  question={`¿Confirmas que el tipo general de IVA de ${tipo.nombre} es el ${pctText(bp ?? tipo.tabla)}? Quedará en la auditoría y se propondrá en las facturas.`}
                  confirmLabel="Confirmar"
                  danger={false}
                  className="text-sky-800 underline"
                  onConfirm={async () => {
                    await ossConfirmarTipo(code, bp ?? tipo.tabla, true);
                    load();
                  }}
                />
              </span>
            ) : null}
            {oss.rate && bp === null && (
              <span className="block text-red-700">Escribe el tipo como «20» o «25,5».</span>
            )}
          </div>
          <p className="col-span-2 text-xs text-stone-600">
            Las líneas van sin IVA español (no sujetas por las reglas de localización) y la factura
            suma el IVA de {tipo?.nombre ?? country}, que se contabiliza en su subcuenta de la 477 y
            no va al 303.
          </p>
        </div>
      )}
      {error && <p className="text-red-700">{error}</p>}
    </div>
  );
}
