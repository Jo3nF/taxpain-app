import { useCallback, useEffect, useState } from 'react';
import {
  type ComunicacionIncobrable,
  type CreditoIncobrable,
  type ExclusionIncobrable,
  errorMessage,
  euros,
  formatDate,
  incobrableClaim,
  incobrableClose,
  incobrableCommunicated,
  incobrableCommunication,
  incobrableIssue,
  incobrableOpen,
  incobrablesList,
  type MedioReclamacion,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { today } from './Invoices';

const ESTADO: Record<CreditoIncobrable['estado'], string> = {
  candidata: 'Impagada',
  abierto: 'En seguimiento',
  rectificado: 'R3 emitida',
  comunicado: 'Comunicada a la AEAT',
  cobrado: 'Cobrada',
  desistido: 'Desistida',
};

const MEDIOS: { value: MedioReclamacion; label: string }[] = [
  { value: 'fehaciente', label: 'Burofax u otro medio fehaciente' },
  { value: 'notarial', label: 'Requerimiento notarial' },
  { value: 'judicial', label: 'Reclamación judicial (demanda, monitorio)' },
];

const EXCLUSIONES: { value: ExclusionIncobrable; label: string }[] = [
  { value: 'ninguna', label: 'Ninguna' },
  { value: 'garantia_real', label: 'Tiene garantía real' },
  { value: 'afianzado', label: 'Afianzado por banco o SGR, o con seguro de crédito' },
  { value: 'vinculada', label: 'Cliente vinculado' },
  { value: 'concurso', label: 'Cliente en concurso (va por R2)' },
];

const input = 'rounded border border-stone-300 px-2 py-1';

function Claim({ c, onDone }: { c: CreditoIncobrable; onDone: (m: string) => void }) {
  const [fecha, setFecha] = useState(c.reclamacion_fecha ?? today());
  const [medio, setMedio] = useState<MedioReclamacion>(c.reclamacion_medio ?? 'fehaciente');
  const [ref, setRef] = useState(c.reclamacion_ref ?? '');
  return (
    <div className="mt-2 flex flex-wrap items-end gap-2">
      <label>
        Reclamado el
        <input
          type="date"
          className={`${input} ml-1`}
          value={fecha}
          onChange={(e) => setFecha(e.target.value)}
        />
      </label>
      <label>
        Medio
        <select
          className={`${input} ml-1`}
          value={medio}
          onChange={(e) => setMedio(e.target.value as MedioReclamacion)}
        >
          {MEDIOS.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
      </label>
      <label>
        Referencia
        <input
          className={`${input} ml-1`}
          value={ref}
          placeholder="n.º de burofax, protocolo…"
          onChange={(e) => setRef(e.target.value)}
        />
      </label>
      <ConfirmAction
        className="text-link underline"
        danger={false}
        label="Anotar la reclamación…"
        question={`Se anotará que reclamaste el cobro de ${c.number} el ${formatDate(fecha)}. Guarda el justificante: hay que adjuntarlo al comunicar la rectificativa.`}
        confirmLabel="Sí, anotarla"
        onConfirm={async () => {
          if (c.id === null) return;
          await incobrableClaim(c.id, fecha, medio, ref, true);
          onDone('Reclamación anotada.');
        }}
      />
    </div>
  );
}

function Communication({ c, onDone }: { c: CreditoIncobrable; onDone: (m: string) => void }) {
  const [datos, setDatos] = useState<ComunicacionIncobrable | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fecha, setFecha] = useState(today());
  const [ref, setRef] = useState('');
  return (
    <div className="mt-2">
      {!datos ? (
        <button
          type="button"
          className="text-link underline"
          onClick={() =>
            c.id !== null &&
            incobrableCommunication(c.id)
              .then(setDatos)
              .catch((e) => setError(errorMessage(e)))
          }
        >
          Datos para comunicarla a la AEAT
        </button>
      ) : (
        <div className="rounded border border-stone-200 p-2">
          <p>{datos.donde}</p>
          {datos.plazo && <p className="font-medium">Plazo: hasta el {formatDate(datos.plazo)}.</p>}
          <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-3">
            {datos.campos.map(([k, v]) => (
              <div key={k} className="contents">
                <dt className="text-stone-600">{k}</dt>
                <dd>{v}</dd>
              </div>
            ))}
          </dl>
          <p className="mt-1 font-medium">Adjunta:</p>
          <ul className="list-disc pl-5">
            {datos.adjuntos.map((a) => (
              <li key={a}>{a}</li>
            ))}
          </ul>
        </div>
      )}
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
      <div className="mt-2 flex flex-wrap items-end gap-2">
        <label>
          Comunicada el
          <input
            type="date"
            className={`${input} ml-1`}
            value={fecha}
            onChange={(e) => setFecha(e.target.value)}
          />
        </label>
        <label>
          Justificante
          <input
            className={`${input} ml-1`}
            value={ref}
            placeholder="n.º de registro o CSV"
            onChange={(e) => setRef(e.target.value)}
          />
        </label>
        <ConfirmAction
          className="text-link underline"
          danger={false}
          label="Anotar la comunicación…"
          question={`Se anotará que comunicaste la rectificativa ${c.rectificativa ?? ''} a la AEAT el ${formatDate(fecha)}. TaxPain no la envía: hazlo tú en la sede electrónica.`}
          confirmLabel="Sí, anotarla"
          onConfirm={async () => {
            if (c.id === null) return;
            await incobrableCommunicated(c.id, fecha, ref, true);
            onDone('Comunicación anotada.');
          }}
        />
      </div>
    </div>
  );
}

function Row({ c, onDone }: { c: CreditoIncobrable; onDone: (m: string) => void }) {
  const [plazo, setPlazo] = useState(String(c.plazo_meses));
  const [exclusion, setExclusion] = useState<ExclusionIncobrable>(c.exclusion);
  const [fecha, setFecha] = useState(today());
  return (
    <li className="py-3 text-sm">
      <p className="font-medium">
        {c.number} · {c.cliente} · {euros(c.pendiente)} pendiente
        <span className="pill pill-stone ml-2">{ESTADO[c.estado]}</span>
      </p>
      <p className="text-stone-600">
        Factura del {formatDate(c.issue_date)} (devengo {formatDate(c.devengo)}). IVA que se
        recupera: {euros(c.iva_pendiente)} (base {euros(c.base_pendiente)}). Plazo de{' '}
        {c.plazo_meses === 6 ? 'seis meses' : 'un año'}: se cumple el{' '}
        {formatDate(c.calendario.desde)}; la R3 se emite como tarde el{' '}
        {formatDate(c.calendario.limite_rectificar)}.
      </p>
      {c.avisos.map((a) => (
        <p key={a} className="text-amber-800">
          {a}
        </p>
      ))}
      {c.rectificativa && (
        <p>
          Rectificativa {c.rectificativa}
          {c.rectificativa_fecha && ` del ${formatDate(c.rectificativa_fecha)}`}
          {c.comunicada_el && `, comunicada el ${formatDate(c.comunicada_el)}`}.
        </p>
      )}
      <p className="text-stone-600">
        {c.deterioro ??
          'Al cierre, el deterioro de la base (694/490) se propone en Contabilidad › Ajustes de cierre; con la R3 emitida, sólo por la base.'}
      </p>
      {(c.estado === 'candidata' || c.estado === 'abierto') && (
        <ul className="mt-1">
          {c.requisitos.map((r) => (
            <li key={r.clave} className={r.cumplido ? 'text-green-800' : 'text-red-700'}>
              {r.cumplido ? '✓' : '✗'} {r.texto}
            </li>
          ))}
        </ul>
      )}
      {c.estado === 'candidata' && (
        <div className="mt-2 flex flex-wrap items-end gap-2">
          <label>
            Plazo
            <select
              className={`${input} ml-1`}
              value={plazo}
              onChange={(e) => setPlazo(e.target.value)}
            >
              <option value="6">Seis meses (volumen de operaciones hasta 6.010.121,04 €)</option>
              <option value="12">Un año</option>
            </select>
          </label>
          <label>
            No procede si
            <select
              className={`${input} ml-1`}
              value={exclusion}
              onChange={(e) => setExclusion(e.target.value as ExclusionIncobrable)}
            >
              {EXCLUSIONES.map((x) => (
                <option key={x.value} value={x.value}>
                  {x.label}
                </option>
              ))}
            </select>
          </label>
          <ConfirmAction
            className="text-link underline"
            danger={false}
            label="Seguir como incobrable…"
            question={`Se abrirá el seguimiento de ${c.number} como crédito incobrable, con su calendario de plazos y avisos.`}
            confirmLabel="Sí, seguirla"
            onConfirm={async () => {
              await incobrableOpen(c.invoice_id, Number(plazo), exclusion, true);
              onDone(`${c.number} en seguimiento.`);
            }}
          />
        </div>
      )}
      {c.estado === 'abierto' && <Claim c={c} onDone={onDone} />}
      {c.estado === 'abierto' && (
        <div className="mt-2 flex flex-wrap items-end gap-2">
          <label>
            Fecha de la R3
            <input
              type="date"
              className={`${input} ml-1`}
              value={fecha}
              onChange={(e) => setFecha(e.target.value)}
            />
          </label>
          <ConfirmAction
            className="btn-primary rounded px-3 py-1"
            danger={false}
            disabled={!c.puede_rectificar}
            label="Emitir la rectificativa R3…"
            question={`Se emitirá la rectificativa R3 de ${c.number} por −${euros(c.base_pendiente)} de base y −${euros(c.iva_pendiente)} de IVA, en la serie de rectificativas y con su registro VERI*FACTU si lo tienes activo. No se puede deshacer. Después, comunícala a la AEAT en el plazo de un mes.`}
            confirmLabel="Sí, emitir la R3"
            onConfirm={async () => {
              if (c.id === null) return;
              const out = await incobrableIssue(c.id, fecha, true);
              onDone(`Emitida ${out.number}. Comunícala a la AEAT en el plazo de un mes.`);
            }}
          />
          <ConfirmAction
            className="text-link underline"
            danger={false}
            label="Se ha cobrado…"
            question={`Se cerrará el seguimiento de ${c.number} porque se ha cobrado.`}
            confirmLabel="Sí, cerrarlo"
            onConfirm={async () => {
              if (c.id === null) return;
              onDone(await incobrableClose(c.id, 'cobrado', today(), '', true));
            }}
          />
        </div>
      )}
      {c.estado === 'rectificado' && <Communication c={c} onDone={onDone} />}
      {(c.estado === 'rectificado' || c.estado === 'comunicado') && (
        <ConfirmAction
          className="mt-2 text-link underline"
          danger={false}
          label="Desistí de la reclamación o hubo acuerdo de cobro…"
          question="Si desistes de la reclamación judicial o llegas a un acuerdo de cobro con el cliente, hay que rectificar al alza en el mes siguiente. Se anotará con la fecha de hoy."
          confirmLabel="Sí, anotarlo"
          onConfirm={async () => {
            if (c.id === null) return;
            onDone(await incobrableClose(c.id, 'desistido', today(), '', true));
          }}
        />
      )}
    </li>
  );
}

/** Cobros y pagos › Créditos incobrables (art. 80.Cuatro LIVA). */
export function Incobrables() {
  const [list, setList] = useState<CreditoIncobrable[] | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const load = useCallback(() => {
    incobrablesList(today())
      .then(setList)
      .catch((e) => setMessage(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  const done = (m: string) => {
    setMessage(m);
    load();
  };
  return (
    <section className="text-sm">
      <h2 className="text-lg font-semibold">Créditos incobrables</h2>
      <p className="mt-1 text-stone-600">
        Si un cliente no paga, puedes recuperar el IVA que ya ingresaste (art. 80.Cuatro LIVA): pasa
        el plazo de seis meses o un año desde el devengo, reclama el cobro por burofax, notario o
        demanda, emite la rectificativa R3 en los seis meses siguientes y comunícala a la AEAT en el
        mes siguiente a la rectificativa. TaxPain lleva el calendario; la comunicación la haces tú
        en la sede electrónica.
      </p>
      {message && (
        <p role="status" className="mt-2">
          {message}
        </p>
      )}
      {list && list.length === 0 && (
        <p className="mt-3">No hay facturas emitidas vencidas sin cobrar.</p>
      )}
      {list && list.length > 0 && (
        <ul className="mt-3 divide-y divide-stone-200">
          {list.map((c) => (
            <Row key={`${c.invoice_id}-${c.estado}`} c={c} onDone={done} />
          ))}
        </ul>
      )}
    </section>
  );
}
