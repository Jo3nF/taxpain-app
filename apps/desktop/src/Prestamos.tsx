import { useCallback, useEffect, useState } from 'react';
import {
  type ContactRow,
  contactsList,
  errorMessage,
  euros,
  formatDate,
  type Prestamo,
  type PrestamoContraparte,
  type PrestamoCuota,
  type PrestamoMovimiento,
  type PrestamoPago,
  type PrestamoSentido,
  type PrestamoSistema,
  type PrestamoVinculo,
  type PrestamoVista,
  parseEuros,
  plural,
  prestamoAccrue,
  prestamoBankCandidates,
  prestamoContractSave,
  prestamoCreate,
  prestamoDelete,
  prestamoMarkOutside,
  prestamoPostInstallment,
  prestamoPostStart,
  prestamoPropuesta,
  prestamoStartOutside,
  prestamosList,
  prestamosReclassify,
  prestamoUndoAccrual,
  prestamoUndoInstallment,
  prestamoUndoStart,
  prestamoUpdate,
  type Shareholder,
  shareholdersList,
} from './api';
import { ConfirmAction } from './ConfirmAction';
import { localToday } from './JournalEntryForm';

const SISTEMAS: { id: PrestamoSistema; label: string }[] = [
  { id: 'frances', label: 'Francés (cuota constante)' },
  { id: 'lineal', label: 'Lineal (amortización constante)' },
  { id: 'intereses_periodicos', label: 'Intereses periódicos y capital al final' },
  { id: 'al_vencimiento', label: 'Todo al vencimiento' },
];

const VINCULOS: { id: PrestamoVinculo; label: string }[] = [
  { id: 'ninguno', label: 'No vinculada' },
  { id: 'grupo', label: 'Empresa del grupo' },
  { id: 'asociada', label: 'Empresa asociada' },
  { id: 'otra', label: 'Otra parte vinculada (socio del 25 %, administrador…)' },
];

/** «5» / «4,75» → centésimas de punto (500, 475); null si no es un porcentaje válido. */
export function parsePct(s: string): number | null {
  const t = s.trim().replace(',', '.');
  if (!/^\d{1,3}(\.\d{1,2})?$/.test(t)) return null;
  const n = Math.round(Number(t) * 100);
  return n <= 10000 ? n : null;
}

/** 475 → «4,75». */
const pctText = (bp: number) =>
  (bp / 100).toLocaleString('es-ES', { maximumFractionDigits: 2, useGrouping: false });

const entero = (s: string): number | null => (/^\d{1,3}$/.test(s.trim()) ? Number(s) : null);

/** Datos del formulario (texto tal como se teclea). */
interface Form {
  nombre: string;
  sentido: PrestamoSentido;
  contraparte: PrestamoContraparte;
  inicio: string;
  principal: string;
  tipo: string;
  sistema: PrestamoSistema;
  periodicidad: string;
  plazo: string;
  carencia: string;
  carencia_tipo: 'capital' | 'total';
  largo: string;
  corto: string;
  intereses: string;
  devengo: string;
  retencion: string;
  notas: string;
}

const vacio = (): Form => ({
  nombre: '',
  sentido: 'recibido',
  contraparte: {
    origen: 'texto',
    ref_id: null,
    nif: '',
    nombre: '',
    provincia: '',
    persona_fisica: false,
    entidad_credito: true,
    vinculo: 'ninguno',
  },
  inicio: localToday(),
  principal: '',
  tipo: '',
  sistema: 'frances',
  periodicidad: '1',
  plazo: '60',
  carencia: '0',
  carencia_tipo: 'capital',
  largo: '',
  corto: '',
  intereses: '',
  devengo: '',
  retencion: '0',
  notas: '',
});

const deFicha = (p: Prestamo): Form => ({
  nombre: p.nombre,
  sentido: p.sentido,
  contraparte: { ...p.contraparte },
  inicio: p.inicio,
  principal: euros(p.principal).replace(/\s?€$/, ''),
  tipo: pctText(p.tipo),
  sistema: p.sistema,
  periodicidad: String(p.periodicidad),
  plazo: String(p.plazo),
  carencia: String(p.carencia),
  carencia_tipo: p.carencia_tipo,
  largo: p.cuentas.largo,
  corto: p.cuentas.corto,
  intereses: p.cuentas.intereses,
  devengo: p.cuentas.devengo,
  retencion: pctText(p.retencion),
  notas: p.notas,
});

/** Ficha a partir del formulario, o el motivo por el que no vale. */
function aFicha(f: Form, id: number): Prestamo | string {
  const principal = parseEuros(f.principal);
  if (!f.nombre.trim()) return 'Ponle un nombre al préstamo.';
  if (!f.contraparte.nombre.trim()) return 'Falta el nombre de la contraparte.';
  if (principal === null || principal <= 0) return 'Revisa el principal (un importe en euros).';
  const tipo = parsePct(f.tipo || '0');
  if (tipo === null) return 'Revisa el interés anual: un porcentaje como 4 o 4,75.';
  const retencion = parsePct(f.retencion || '0');
  if (retencion === null) return 'Revisa la retención: un porcentaje como 19 o 0.';
  const plazo = entero(f.plazo);
  const carencia = entero(f.carencia || '0');
  if (plazo === null || plazo <= 0) return 'Revisa el plazo (meses).';
  if (carencia === null) return 'Revisa la carencia (meses).';
  if (!f.largo || !f.corto || !f.intereses || !f.devengo) return 'Faltan las cuentas.';
  return {
    id,
    nombre: f.nombre.trim(),
    sentido: f.sentido,
    contraparte: {
      ...f.contraparte,
      nif: f.contraparte.nif.trim(),
      nombre: f.contraparte.nombre.trim(),
      provincia: f.contraparte.provincia.trim(),
    },
    inicio: f.inicio,
    principal,
    tipo,
    sistema: f.sistema,
    periodicidad: Number(f.periodicidad),
    plazo,
    carencia: f.sistema === 'al_vencimiento' ? 0 : carencia,
    carencia_tipo: f.carencia_tipo,
    cuentas: {
      largo: f.largo.trim(),
      corto: f.corto.trim(),
      intereses: f.intereses.trim(),
      devengo: f.devengo.trim(),
    },
    retencion,
    notas: f.notas,
  };
}

/** Alta y edición de la ficha. */
function PrestamoForm({
  inicial,
  bloqueado,
  onSaved,
  onCancel,
}: {
  inicial: Prestamo | null;
  /** Con asientos: sólo se cambian el nombre, la contraparte, la retención y las notas. */
  bloqueado: boolean;
  onSaved: (msg: string) => void;
  onCancel?: () => void;
}) {
  const [f, setF] = useState<Form>(() => (inicial ? deFicha(inicial) : vacio()));
  const [motivo, setMotivo] = useState<string>('');
  const [socios, setSocios] = useState<Shareholder[]>([]);
  const [contactos, setContactos] = useState<ContactRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const c = f.contraparte;
  const setC = (patch: Partial<PrestamoContraparte>) =>
    setF((x) => ({ ...x, contraparte: { ...x.contraparte, ...patch } }));

  useEffect(() => {
    shareholdersList()
      .then(setSocios)
      .catch(() => setSocios([]));
  }, []);
  const conContactos = f.contraparte.origen === 'contacto';
  useEffect(() => {
    if (!conContactos) return;
    contactsList(new Date().getFullYear())
      .then(setContactos)
      .catch(() => setContactos([]));
  }, [conContactos]);

  // Al cambiar el sentido o el tipo de contraparte, se proponen cuentas y retención (en una
  // ficha nueva o sin asientos; el usuario las puede cambiar después).
  const { sentido } = f;
  const { vinculo, entidad_credito, persona_fisica } = c;
  useEffect(() => {
    if (bloqueado) return;
    let vivo = true;
    prestamoPropuesta(sentido, {
      origen: 'texto',
      ref_id: null,
      nif: '',
      nombre: '',
      provincia: '',
      persona_fisica,
      entidad_credito,
      vinculo,
    })
      .then((p) => {
        if (!vivo) return;
        setMotivo(p.motivo);
        setF((x) =>
          inicial &&
          x.sentido === inicial.sentido &&
          x.contraparte.vinculo === inicial.contraparte.vinculo &&
          x.contraparte.entidad_credito === inicial.contraparte.entidad_credito &&
          x.contraparte.persona_fisica === inicial.contraparte.persona_fisica
            ? x
            : {
                ...x,
                largo: p.cuentas.largo,
                corto: p.cuentas.corto,
                intereses: p.cuentas.intereses,
                devengo: p.cuentas.devengo,
                retencion: pctText(p.retencion),
              },
        );
      })
      .catch(() => {});
    return () => {
      vivo = false;
    };
  }, [sentido, vinculo, entidad_credito, persona_fisica, bloqueado, inicial]);

  const guardar = async () => {
    const p = aFicha(f, inicial?.id ?? 0);
    if (typeof p === 'string') {
      setError(p);
      return;
    }
    try {
      if (inicial) {
        await prestamoUpdate(inicial.id, p);
        onSaved(`Préstamo «${p.nombre}» guardado.`);
      } else {
        await prestamoCreate(p);
        setF(vacio());
        onSaved(
          `Préstamo «${p.nombre}» dado de alta. Contabiliza ahora la ${p.sentido === 'concedido' ? 'entrega' : 'recepción'} del principal (o márcala como ya contabilizada si ya estaba en el diario).`,
        );
      }
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  return (
    <div className="form-grid mt-3">
      <label className="span-2">
        Nombre del préstamo
        <input value={f.nombre} onChange={(e) => setF({ ...f, nombre: e.target.value })} />
      </label>
      <label>
        Sentido
        <select
          value={f.sentido}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, sentido: e.target.value as PrestamoSentido })}
        >
          <option value="recibido">Recibido (nos prestan)</option>
          <option value="concedido">Concedido (prestamos nosotros)</option>
        </select>
      </label>
      <label>
        Contraparte
        <select
          value={c.origen}
          onChange={(e) =>
            setC({ origen: e.target.value as PrestamoContraparte['origen'], ref_id: null })
          }
        >
          <option value="texto">Banco u otro (escribir)</option>
          <option value="socio">Un socio</option>
          <option value="participada">Una participada</option>
          <option value="contacto">Un contacto</option>
        </select>
      </label>
      {c.origen === 'socio' && (
        <label className="span-2">
          Socio
          <select
            value={c.ref_id ?? ''}
            onChange={(e) => {
              const s = socios.find((x) => x.id === Number(e.target.value));
              if (!s) return;
              setC({
                ref_id: s.id,
                nif: s.nif,
                nombre: s.name,
                provincia: s.province,
                persona_fisica: !s.legal_person,
                entidad_credito: false,
                vinculo: s.pct >= 250_000 ? 'otra' : c.vinculo,
              });
            }}
          >
            <option value="">{socios.length ? 'Elige…' : 'No hay socios en la ficha'}</option>
            {socios.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name} ({s.nif})
              </option>
            ))}
          </select>
        </label>
      )}
      {c.origen === 'contacto' && (
        <label className="span-2">
          Contacto
          <select
            value={c.ref_id ?? ''}
            onChange={(e) => {
              const x = contactos.find((k) => k.id === Number(e.target.value));
              if (!x) return;
              setC({ ref_id: x.id, nif: x.tax_id, nombre: x.name });
            }}
          >
            <option value="">{contactos.length ? 'Elige…' : 'No hay contactos'}</option>
            {contactos
              .filter((k) => k.id !== null)
              .map((k) => (
                <option key={k.key} value={k.id ?? ''}>
                  {k.name} ({k.tax_id})
                </option>
              ))}
          </select>
        </label>
      )}
      <label>
        NIF de la contraparte
        <input value={c.nif} onChange={(e) => setC({ nif: e.target.value })} />
      </label>
      <label>
        Nombre de la contraparte
        <input value={c.nombre} onChange={(e) => setC({ nombre: e.target.value })} />
      </label>
      <label>
        Provincia (01–53)
        <input value={c.provincia} onChange={(e) => setC({ provincia: e.target.value })} />
      </label>
      <label>
        Vínculo
        <select
          value={c.vinculo}
          disabled={bloqueado}
          onChange={(e) => setC({ vinculo: e.target.value as PrestamoVinculo })}
        >
          {VINCULOS.map((v) => (
            <option key={v.id} value={v.id}>
              {v.label}
            </option>
          ))}
        </select>
      </label>
      <label>
        <input
          type="checkbox"
          checked={c.entidad_credito}
          disabled={bloqueado}
          onChange={(e) =>
            setC({
              entidad_credito: e.target.checked,
              persona_fisica: e.target.checked ? false : c.persona_fisica,
            })
          }
        />
        Es un banco
      </label>
      <label>
        <input
          type="checkbox"
          checked={c.persona_fisica}
          disabled={bloqueado}
          onChange={(e) =>
            setC({
              persona_fisica: e.target.checked,
              entidad_credito: e.target.checked ? false : c.entidad_credito,
            })
          }
        />
        Es una persona física
      </label>
      <label>
        Fecha de inicio
        <input
          type="date"
          value={f.inicio}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, inicio: e.target.value })}
        />
      </label>
      <label>
        Principal
        <input
          value={f.principal}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, principal: e.target.value })}
        />
      </label>
      <label>
        Interés nominal anual %
        <input
          value={f.tipo}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, tipo: e.target.value })}
        />
      </label>
      <label>
        Sistema
        <select
          value={f.sistema}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, sistema: e.target.value as PrestamoSistema })}
        >
          {SISTEMAS.map((s) => (
            <option key={s.id} value={s.id}>
              {s.label}
            </option>
          ))}
        </select>
      </label>
      {f.sistema !== 'al_vencimiento' && (
        <label>
          Cuotas
          <select
            value={f.periodicidad}
            disabled={bloqueado}
            onChange={(e) => setF({ ...f, periodicidad: e.target.value })}
          >
            <option value="1">Mensuales</option>
            <option value="3">Trimestrales</option>
            <option value="6">Semestrales</option>
            <option value="12">Anuales</option>
          </select>
        </label>
      )}
      <label>
        Plazo (meses)
        <input
          value={f.plazo}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, plazo: e.target.value })}
        />
      </label>
      {f.sistema !== 'al_vencimiento' && (
        <>
          <label>
            Carencia (meses)
            <input
              value={f.carencia}
              disabled={bloqueado}
              onChange={(e) => setF({ ...f, carencia: e.target.value })}
            />
          </label>
          <label>
            Tipo de carencia
            <select
              value={f.carencia_tipo}
              disabled={bloqueado}
              onChange={(e) => setF({ ...f, carencia_tipo: e.target.value as 'capital' | 'total' })}
            >
              <option value="capital">De capital (se pagan los intereses)</option>
              <option value="total">Total (los intereses se capitalizan)</option>
            </select>
          </label>
        </>
      )}
      <label>
        Cuenta a largo plazo
        <input
          value={f.largo}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, largo: e.target.value })}
        />
      </label>
      <label>
        Cuenta a corto plazo
        <input
          value={f.corto}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, corto: e.target.value })}
        />
      </label>
      <label>
        Cuenta de intereses
        <input
          value={f.intereses}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, intereses: e.target.value })}
        />
      </label>
      <label>
        Cuenta de intereses devengados
        <input
          value={f.devengo}
          disabled={bloqueado}
          onChange={(e) => setF({ ...f, devengo: e.target.value })}
        />
      </label>
      <label>
        Retención sobre los intereses %
        <input value={f.retencion} onChange={(e) => setF({ ...f, retencion: e.target.value })} />
      </label>
      <label className="span-2">
        Notas
        <input value={f.notas} onChange={(e) => setF({ ...f, notas: e.target.value })} />
      </label>
      <p className="span-2 text-stone-600">
        {motivo}{' '}
        {bloqueado
          ? 'El préstamo ya tiene asientos: sus condiciones y sus cuentas no se cambian (deshaz antes las cuotas y el alta).'
          : 'Para una cuenta corriente con el socio, pon la 551 en las dos cuentas de capital.'}
      </p>
      <div className="form-actions">
        {onCancel && (
          <button
            type="button"
            className="rounded border border-stone-300 px-3 py-1"
            onClick={onCancel}
          >
            Cancelar
          </button>
        )}
        <button type="button" className="rounded btn-primary px-3 py-1" onClick={guardar}>
          {inicial ? 'Guardar cambios' : 'Dar de alta'}
        </button>
      </div>
      {error && (
        <p role="alert" className="span-2 text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

/** Elige con qué se paga o cobra: un movimiento del banco que cuadra o una cuenta de tesorería. */
function PagoSelector({
  id,
  n,
  onChange,
}: {
  id: number;
  /** Cuota (0: el alta). */
  n: number;
  onChange: (p: PrestamoPago) => void;
}) {
  const [movs, setMovs] = useState<PrestamoMovimiento[]>([]);
  const [eleccion, setEleccion] = useState<string>('cuenta');
  const [cuenta, setCuenta] = useState('572');
  useEffect(() => {
    let vivo = true;
    prestamoBankCandidates(id, n)
      .then((m) => {
        if (!vivo) return;
        setMovs(m);
        const first = m[0];
        if (first) {
          setEleccion(String(first.id));
          onChange({ tesoreria: null, bank_tx: first.id });
        } else {
          onChange({ tesoreria: '572', bank_tx: null });
        }
      })
      .catch(() => onChange({ tesoreria: '572', bank_tx: null }));
    return () => {
      vivo = false;
    };
  }, [id, n, onChange]);
  return (
    <div className="flex flex-wrap items-end gap-2">
      <label>
        Pago o cobro
        <select
          value={eleccion}
          onChange={(e) => {
            setEleccion(e.target.value);
            onChange(
              e.target.value === 'cuenta'
                ? { tesoreria: cuenta, bank_tx: null }
                : { tesoreria: null, bank_tx: Number(e.target.value) },
            );
          }}
        >
          {movs.map((m) => (
            <option key={m.id} value={m.id}>
              Movimiento del banco del {formatDate(m.fecha)}: {euros(m.importe)} · {m.concepto}
            </option>
          ))}
          <option value="cuenta">Contra una cuenta (sin movimiento del banco)</option>
        </select>
      </label>
      {eleccion === 'cuenta' && (
        <label>
          Cuenta de tesorería
          <input
            value={cuenta}
            onChange={(e) => {
              setCuenta(e.target.value);
              onChange({ tesoreria: e.target.value, bank_tx: null });
            }}
          />
        </label>
      )}
    </div>
  );
}

function Cuadro({ cuotas }: { cuotas: PrestamoCuota[] }) {
  const estado = { pendiente: 'Pendiente', registrada: 'Registrada', fuera: 'Ya contabilizada' };
  return (
    <div className="overflow-x-auto">
      <table className="mt-2 w-full text-xs">
        <thead>
          <tr className="text-left text-stone-600">
            <th className="px-2">N.º</th>
            <th className="px-2">Vencimiento</th>
            <th className="px-2 text-right">Capital</th>
            <th className="px-2 text-right">Intereses</th>
            <th className="px-2 text-right">Cuota</th>
            <th className="px-2 text-right">Retención</th>
            <th className="px-2 text-right">Pendiente</th>
            <th className="px-2">Estado</th>
          </tr>
        </thead>
        <tbody>
          {cuotas.map((q) => (
            <tr key={q.n} className="border-t border-stone-100">
              <td className="px-2">{q.n}</td>
              <td className="px-2">{formatDate(q.fecha)}</td>
              <td className="px-2 text-right tabular-nums">{euros(q.capital)}</td>
              <td className="px-2 text-right tabular-nums">
                {euros(q.intereses)}
                {q.capitalizados > 0 && ' (capitalizados)'}
              </td>
              <td className="px-2 text-right tabular-nums">{euros(q.pago)}</td>
              <td className="px-2 text-right tabular-nums">{euros(q.retencion)}</td>
              <td className="px-2 text-right tabular-nums">{euros(q.pendiente)}</td>
              <td className="px-2">
                {estado[q.estado]}
                {q.bank_tx !== null && ' · conciliada'}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Ficha de un préstamo con sus acciones. */
function Tarjeta({
  v,
  year,
  onDone,
}: {
  v: PrestamoVista;
  year: number;
  onDone: (msg: string) => void;
}) {
  const p = v.prestamo;
  const concedido = p.sentido === 'concedido';
  const [pagoAlta, setPagoAlta] = useState<PrestamoPago>({ tesoreria: '572', bank_tx: null });
  const [pagoCuota, setPagoCuota] = useState<PrestamoPago>({ tesoreria: '572', bank_tx: null });
  const [fechaDevengo, setFechaDevengo] = useState(`${year}-12-31`);
  const [hasta, setHasta] = useState(`${year - 1}-12-31`);
  const [editar, setEditar] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const siguiente = v.cuotas.find((q) => q.estado === 'pendiente');
  const registradas = v.cuotas.filter((q) => q.estado !== 'pendiente');
  const ultima = registradas[registradas.length - 1];
  const conAsientos = v.alta === 'contabilizada' || registradas.length > 0 || v.devengos.length > 0;
  const pago = (q: PrestamoCuota) => (q.neto === 0 ? 'sin movimiento de dinero' : euros(q.neto));
  const onAlta = useCallback((x: PrestamoPago) => setPagoAlta(x), []);
  const onCuota = useCallback((x: PrestamoPago) => setPagoCuota(x), []);

  return (
    <div className="space-y-2 rounded border border-stone-200 p-3">
      <p className="font-medium">
        {p.nombre}{' '}
        {p.contraparte.vinculo !== 'ninguno' && (
          <span className="rounded bg-amber-100 px-1 text-xs text-amber-900">Vinculada</span>
        )}
      </p>
      <p>
        {concedido ? 'Concedido a' : 'Recibido de'} {p.contraparte.nombre}
        {p.contraparte.nif && ` (${p.contraparte.nif})`} · {euros(p.principal)} al {pctText(p.tipo)}{' '}
        % · {SISTEMAS.find((s) => s.id === p.sistema)?.label.toLowerCase()} ·{' '}
        {plural(v.cuotas.length, 'cuota')} · retención {pctText(p.retencion)} %
      </p>
      <p>
        Saldo vivo hoy: <strong>{euros(v.saldo_vivo)}</strong> ({euros(v.corto_plazo)} a corto y{' '}
        {euros(v.largo_plazo)} a largo plazo) · registradas {registradas.length}/{v.cuotas.length}
        {v.vencidas > 0 &&
          ` · ${plural(v.vencidas, 'cuota vencida', 'cuotas vencidas')} sin registrar`}
      </p>
      {v.avisos.length > 0 && (
        <ul className="list-disc space-y-1 rounded border border-amber-300 bg-amber-50 p-2 pl-6 text-stone-900">
          {v.avisos.map((a) => (
            <li key={a}>{a}</li>
          ))}
        </ul>
      )}

      {v.alta === 'pendiente' && (
        <div className="space-y-1 rounded border border-stone-200 p-2">
          <p>
            Falta contabilizar la {concedido ? 'entrega' : 'recepción'} del principal del{' '}
            {formatDate(p.inicio)}.
          </p>
          <PagoSelector id={p.id} n={0} onChange={onAlta} />
          <div className="flex flex-wrap gap-3">
            <ConfirmAction
              label={`Contabilizar la ${concedido ? 'entrega' : 'recepción'} del principal`}
              className="rounded border border-stone-300 px-2 py-0.5"
              danger={false}
              confirmLabel="Sí, contabilizar"
              question={
                <>
                  Asiento del {formatDate(p.inicio)}: {euros(p.principal)}{' '}
                  {concedido
                    ? `a las cuentas ${p.cuentas.largo}/${p.cuentas.corto} contra la tesorería`
                    : `de la tesorería contra las cuentas ${p.cuentas.largo}/${p.cuentas.corto}`}
                  {pagoAlta.bank_tx !== null
                    ? ', conciliado con el movimiento del banco elegido'
                    : ` (cuenta ${pagoAlta.tesoreria ?? ''})`}
                  . Queda en la auditoría.
                </>
              }
              onConfirm={async () => {
                await prestamoPostStart(p.id, pagoAlta, true);
                onDone(`Alta de «${p.nombre}» contabilizada.`);
              }}
            />
            <button
              type="button"
              className="rounded border border-stone-300 px-2 py-0.5"
              onClick={async () => {
                try {
                  await prestamoStartOutside(p.id, true);
                  onDone(
                    `«${p.nombre}»: el alta queda como ya contabilizada (saldos iniciales o diario del gestor).`,
                  );
                } catch (e) {
                  setError(errorMessage(e));
                }
              }}
            >
              Ya está en el diario
            </button>
          </div>
        </div>
      )}
      {v.alta === 'contabilizada' && (
        <p className="text-stone-600">
          Alta contabilizada{v.alta_bank_tx !== null && ' y conciliada con el banco'}.{' '}
          {registradas.length === 0 && v.devengos.length === 0 ? (
            <ConfirmAction
              label="Deshacer el alta"
              confirmLabel="Sí, deshacer el alta"
              question={
                <>
                  Se borra el asiento del alta de «{p.nombre}»
                  {v.alta_bank_tx !== null && ' y su movimiento del banco vuelve a estar pendiente'}
                  . Queda en la auditoría.
                </>
              }
              onConfirm={async () => {
                await prestamoUndoStart(p.id, true);
                onDone(`Alta de «${p.nombre}» deshecha.`);
              }}
            />
          ) : null}
        </p>
      )}
      {v.alta === 'fuera' && (
        <p className="text-stone-600">
          Alta ya contabilizada fuera de TaxPain.{' '}
          {registradas.length === 0 && (
            <button
              type="button"
              className="underline"
              onClick={async () => {
                try {
                  await prestamoStartOutside(p.id, false);
                  onDone(`«${p.nombre}»: el alta vuelve a estar pendiente.`);
                } catch (e) {
                  setError(errorMessage(e));
                }
              }}
            >
              No, está pendiente
            </button>
          )}
        </p>
      )}

      {v.alta !== 'pendiente' && siguiente && (
        <div className="space-y-1 rounded border border-stone-200 p-2">
          <p>
            Siguiente: cuota {siguiente.n} del {formatDate(siguiente.fecha)} · capital{' '}
            {euros(siguiente.capital)} · intereses {euros(siguiente.intereses)}
            {siguiente.retencion > 0 && ` · retención ${euros(siguiente.retencion)}`}
            {siguiente.devengado > 0 && ` · ya devengados ${euros(siguiente.devengado)}`} ·{' '}
            {concedido ? 'se cobra' : 'se paga'} {pago(siguiente)}
          </p>
          {siguiente.neto !== 0 && (
            <PagoSelector key={siguiente.n} id={p.id} n={siguiente.n} onChange={onCuota} />
          )}
          <ConfirmAction
            label={`Registrar cuota ${siguiente.n}`}
            className="rounded border border-stone-300 px-2 py-0.5"
            danger={false}
            confirmLabel="Sí, registrar la cuota"
            question={
              <>
                Asiento del {formatDate(siguiente.fecha)}: capital {euros(siguiente.capital)} a la{' '}
                {p.cuentas.corto}, intereses {euros(siguiente.intereses)} a la {p.cuentas.intereses}
                {siguiente.devengado > 0 &&
                  ` (de ellos, ${euros(siguiente.devengado)} ya devengados en la ${p.cuentas.devengo})`}
                {siguiente.retencion > 0 &&
                  `, retención ${euros(siguiente.retencion)} a la ${concedido ? '473' : '4751'}`}
                {siguiente.neto !== 0 &&
                  (pagoCuota.bank_tx !== null
                    ? ' y el neto conciliado con el movimiento del banco elegido'
                    : ` y el neto contra la ${pagoCuota.tesoreria ?? ''}`)}
                . Queda en la auditoría.
              </>
            }
            onConfirm={async () => {
              await prestamoPostInstallment(p.id, siguiente.n, pagoCuota, true);
              onDone(
                `Cuota ${siguiente.n} de «${p.nombre}» registrada${siguiente.retencion > 0 && !concedido ? ': su retención va al 123 del trimestre y al 193' : ''}.`,
              );
            }}
          />
        </div>
      )}

      <div className="flex flex-wrap items-end gap-3">
        {v.alta !== 'pendiente' && siguiente && (
          <>
            <label>
              Devengar a fecha
              <input
                type="date"
                value={fechaDevengo}
                onChange={(e) => setFechaDevengo(e.target.value)}
              />
            </label>
            <ConfirmAction
              label="Devengar intereses a fecha"
              className="rounded border border-stone-300 px-2 py-0.5"
              danger={false}
              confirmLabel="Sí, devengar"
              question={
                <>
                  Se contabilizan a {formatDate(fechaDevengo)} los intereses de la cuota en curso
                  desde el último vencimiento (en proporción a los días),{' '}
                  {concedido
                    ? `${p.cuentas.devengo} contra ${p.cuentas.intereses}`
                    : `${p.cuentas.intereses} contra ${p.cuentas.devengo}`}
                  . Al registrar la cuota, lo devengado se salda. Queda en la auditoría.
                </>
              }
              onConfirm={async () => {
                const r = await prestamoAccrue(p.id, fechaDevengo, true);
                onDone(
                  r.length === 0
                    ? `«${p.nombre}»: nada que devengar a ${formatDate(fechaDevengo)}.`
                    : `«${p.nombre}»: ${euros(r[0]?.importe ?? 0)} de intereses devengados a ${formatDate(fechaDevengo)} (cuota ${r[0]?.n}).`,
                );
              }}
            />
          </>
        )}
        {ultima && (
          <ConfirmAction
            label={`Deshacer la cuota ${ultima.n}`}
            confirmLabel="Sí, deshacer la cuota"
            question={
              <>
                {ultima.estado === 'registrada'
                  ? `Se borra el asiento de la cuota ${ultima.n}`
                  : `La cuota ${ultima.n} deja de estar marcada como ya contabilizada`}
                {ultima.bank_tx !== null && ' y su movimiento del banco vuelve a estar pendiente'}.
                Queda en la auditoría.
              </>
            }
            onConfirm={async () => {
              await prestamoUndoInstallment(p.id, ultima.n, true);
              onDone(`Cuota ${ultima.n} de «${p.nombre}» deshecha.`);
            }}
          />
        )}
      </div>

      {v.devengos.length > 0 && (
        <ul className="text-stone-700">
          {v.devengos.map((d) => (
            <li key={d.id} className="flex flex-wrap items-center gap-2">
              Devengados {euros(d.importe)} a {formatDate(d.fecha)} (cuota {d.n})
              {v.cuotas.find((q) => q.n === d.n)?.estado === 'pendiente' && (
                <ConfirmAction
                  label="Deshacer"
                  ariaLabel={`Deshacer el devengo del ${formatDate(d.fecha)}`}
                  confirmLabel="Sí, deshacer el devengo"
                  question={<>Se borra el asiento del devengo. Queda en la auditoría.</>}
                  onConfirm={async () => {
                    await prestamoUndoAccrual(d.id, true);
                    onDone('Devengo deshecho.');
                  }}
                />
              )}
            </li>
          ))}
        </ul>
      )}

      <details>
        <summary>Cuadro de amortización</summary>
        <Cuadro cuotas={v.cuotas} />
      </details>

      {siguiente && (
        <details>
          <summary>Cuotas ya contabilizadas fuera de TaxPain</summary>
          <p className="mt-1 text-stone-600">
            Si las cuotas anteriores ya están en el diario (del gestor o en los saldos iniciales),
            márcalas sin hacer asiento. Cuentan para el 123 y el 193 con su retención.
          </p>
          <div className="flex flex-wrap items-end gap-2">
            <label>
              Hasta el
              <input type="date" value={hasta} onChange={(e) => setHasta(e.target.value)} />
            </label>
            <ConfirmAction
              label="Marcar como ya contabilizadas"
              className="rounded border border-stone-300 px-2 py-0.5"
              danger={false}
              confirmLabel="Sí, marcarlas"
              question={
                <>
                  Las cuotas pendientes de «{p.nombre}» que vencen hasta el {formatDate(hasta)} se
                  marcan como ya contabilizadas, sin asiento. Queda en la auditoría.
                </>
              }
              onConfirm={async () => {
                const n = await prestamoMarkOutside(p.id, hasta, true);
                onDone(
                  `«${p.nombre}»: ${plural(n, 'cuota marcada', 'cuotas marcadas')} como ya contabilizadas.`,
                );
              }}
            />
          </div>
        </details>
      )}

      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          className="rounded border border-stone-300 px-2 py-0.5"
          onClick={() => setEditar(!editar)}
        >
          {editar ? 'Cerrar la ficha' : 'Editar la ficha'}
        </button>
        <button
          type="button"
          className="rounded border border-stone-300 px-2 py-0.5"
          onClick={async () => {
            try {
              const path = await prestamoContractSave(p.id, p.nombre);
              if (path)
                onDone(`Contrato guardado en ${path}: revísalo y complétalo antes de firmarlo.`);
            } catch (e) {
              setError(errorMessage(e));
            }
          }}
        >
          Contrato de préstamo (RTF)…
        </button>
        {v.alta !== 'contabilizada' &&
          !v.cuotas.some((q) => q.estado === 'registrada') &&
          v.devengos.length === 0 && (
            <ConfirmAction
              label="Borrar el préstamo"
              confirmLabel="Sí, borrar el préstamo"
              question={
                <>
                  Se borra la ficha de «{p.nombre}» dada de alta por error (no tiene asientos; las
                  cuotas marcadas como ya contabilizadas se van con ella). Queda en la auditoría.
                </>
              }
              onConfirm={async () => {
                await prestamoDelete(p.id, true);
                onDone(`Préstamo «${p.nombre}» borrado.`);
              }}
            />
          )}
      </div>
      {editar && (
        <PrestamoForm
          inicial={p}
          bloqueado={conAsientos}
          onSaved={(m) => {
            setEditar(false);
            onDone(m);
          }}
          onCancel={() => setEditar(false)}
        />
      )}
      {error && (
        <p role="alert" className="text-red-700">
          {error}
        </p>
      )}
    </div>
  );
}

/** Contabilidad → Préstamos: concedidos y recibidos, con su cuadro, cuotas, devengo y cierre. */
export function Prestamos({ year }: { year: number }) {
  const [list, setList] = useState<PrestamoVista[] | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [fecha, setFecha] = useState(`${year}-12-31`);
  const load = useCallback(() => {
    prestamosList()
      .then(setList)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);
  useEffect(() => setFecha(`${year}-12-31`), [year]);
  const done = (m: string) => {
    setMsg(m);
    load();
  };
  const vivos = (list ?? []).filter((v) => v.saldo_vivo > 0);
  const total = (s: PrestamoSentido) =>
    vivos.filter((v) => v.prestamo.sentido === s).reduce((a, v) => a + v.saldo_vivo, 0);

  return (
    <section aria-label="Préstamos" className="mt-4 space-y-3 text-sm">
      <h3 className="font-medium">Préstamos</h3>
      <p className="text-stone-600">
        Préstamos concedidos (a socios, participadas o terceros) y recibidos (de bancos, socios u
        otros): su cuadro, cada cuota con su retención, el devengo de intereses al cierre y la deuda
        o el crédito a corto y a largo plazo.
      </p>
      {list && list.length > 0 && (
        <p>
          Saldo vivo hoy: concedidos {euros(total('concedido'))} · recibidos{' '}
          {euros(total('recibido'))}
        </p>
      )}
      {list && list.length === 0 && <p>No hay préstamos dados de alta.</p>}
      {list?.map((v) => (
        <Tarjeta key={v.prestamo.id} v={v} year={year} onDone={done} />
      ))}
      {list && list.length > 0 && (
        <div className="flex flex-wrap items-end gap-3 rounded border border-stone-200 p-2">
          <span className="font-medium">Cierre de {year}</span>
          <label>
            Fecha del devengo
            <input type="date" value={fecha} onChange={(e) => setFecha(e.target.value)} />
          </label>
          <ConfirmAction
            label="Devengar los intereses de todos"
            className="rounded border border-stone-300 px-2 py-0.5"
            danger={false}
            confirmLabel="Sí, devengar"
            question={
              <>
                Se contabilizan a {formatDate(fecha)} los intereses devengados y no vencidos de cada
                préstamo con el alta contabilizada (lo ya devengado no se repite). Queda en la
                auditoría.
              </>
            }
            onConfirm={async () => {
              const r = await prestamoAccrue(null, fecha, true);
              done(
                r.length === 0
                  ? `Nada que devengar a ${formatDate(fecha)}.`
                  : `Intereses devengados a ${formatDate(fecha)}: ${r.map((d) => `${d.prestamo} ${euros(d.importe)}`).join('; ')}.`,
              );
            }}
          />
          <ConfirmAction
            label={`Reclasificar al cierre de ${year}`}
            className="rounded border border-stone-300 px-2 py-0.5"
            danger={false}
            confirmLabel="Sí, reclasificar"
            question={
              <>
                Al cierre de {year}, cada cuenta de largo plazo queda con el capital que vence a más
                de doce meses según el cuadro, y el resto pasa a la de corto plazo (un asiento por
                par de cuentas; las cuentas corrientes no se tocan). Si ya está al día, no se hace
                nada. Queda en la auditoría.
              </>
            }
            onConfirm={async () => {
              const n = await prestamosReclassify(year, true);
              done(
                n
                  ? `Capital de los préstamos reclasificado al cierre de ${year} (${plural(n, 'asiento')}).`
                  : `El largo y el corto plazo de los préstamos ya estaban al día al cierre de ${year}.`,
              );
            }}
          />
        </div>
      )}
      <details>
        <summary>Nuevo préstamo</summary>
        <PrestamoForm inicial={null} bloqueado={false} onSaved={done} />
      </details>
      {msg && <p role="status">{msg}</p>}
    </section>
  );
}
