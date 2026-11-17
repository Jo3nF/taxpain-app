import { type FormEvent, useCallback, useEffect, useRef, useState } from 'react';
import {
  avisoDestacado,
  type BancaActualizacion,
  type BancaEstado,
  type BancoDisponible,
  bancaActivarCuenta,
  bancaActualizar,
  bancaAjustes,
  bancaBancos,
  bancaCancelar,
  bancaConectar,
  bancaCredenciales,
  bancaDesconectar,
  bancaEsperar,
  bancaEstado,
  bancaQuitarCredenciales,
  bancaTerminar,
  type ConexionBanco,
  type CuentaConectada,
  errorMessage,
  formatDate,
  money,
  openSede,
  pickPemFile,
  plural,
} from './api';
import { ConfirmAction } from './ConfirmAction';

const input = 'w-full rounded border border-stone-300 px-2 py-1';
const PANEL_ENABLE = 'https://enablebanking.com/sign-in/';

function tipoDe(t: string): string {
  return t === 'business' ? 'empresa' : 'particular';
}

/** «Cuenta ES91 …1332» o, sin IBAN, el nombre que da el banco («Tarjeta empresa»). */
function nombreCuenta(k: CuentaConectada): string {
  const tipo = k.tarjeta ? 'Tarjeta' : 'Cuenta';
  if (k.iban) return `${tipo} ${k.iban}`;
  return /^(tarjeta|cuenta)\b/i.test(k.nombre) ? k.nombre : `${tipo} ${k.nombre}`.trim();
}

function dia(iso: string | null): string {
  return iso ? formatDate(iso.slice(0, 10)) : '';
}

/** Lo que ha pasado con una cuenta al bajar sus movimientos, y lo que conviene revisar. */
export interface LineaActualizacion {
  linea: string;
  avisos: string[];
}

/** Lo que ha pasado con cada cuenta al bajar los movimientos: una línea y sus avisos. */
export function describeActualizacion(a: BancaActualizacion): LineaActualizacion[] {
  if (a.cuentas.length === 0)
    return [{ linea: 'No hay ninguna cuenta activa que descargar.', avisos: [] }];
  return a.cuentas.map((c) => {
    const quien = `${c.banco} ${c.nombre}`;
    const r = c.resultado;
    const avisos = r?.avisos ?? [];
    if (c.error) return { linea: `${quien}: ${c.error}`, avisos };
    if (!r || r.movimientos === 0) return { linea: `${quien}: sin movimientos nuevos`, avisos };
    if (!r.importado) return { linea: `${quien}: no se ha importado (${r.motivo})`, avisos };
    const ya = r.ya_estaban ? ` (${plural(r.ya_estaban, 'ya estaba', 'ya estaban')})` : '';
    const nuevos =
      r.movimientos_nuevos === 0
        ? 'sin movimientos nuevos'
        : plural(r.movimientos_nuevos, 'movimiento nuevo', 'movimientos nuevos');
    return { linea: `${quien}: ${nuevos}${ya}`, avisos };
  });
}

/**
 * Conexión de solo lectura con BBVA y Revolut a través de Enable Banking: configurar la
 * aplicación, autorizar el acceso en el banco y bajar los movimientos, que entran como un
 * extracto más. Sin conexión, cómo descargar los extractos para importarlos.
 */
export function BancosConectados({ onImported }: { onImported: () => void }) {
  const [estado, setEstado] = useState<BancaEstado | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [lineas, setLineas] = useState<LineaActualizacion[]>([]);
  const [busy, setBusy] = useState(false);
  const load = useCallback(() => {
    bancaEstado()
      .then(setEstado)
      .catch((e) => setMsg(errorMessage(e)));
  }, []);
  useEffect(load, [load]);

  const hecho = useCallback(
    (a: BancaActualizacion) => {
      setMsg(null);
      setLineas(describeActualizacion(a));
      onImported();
      load();
    },
    [onImported, load],
  );

  if (!estado) {
    return msg ? (
      <p role="alert" className="text-sm text-red-700">
        {msg}
      </p>
    ) : null;
  }
  const listo = estado.ajustes.aceptado && estado.clave;
  return (
    <section
      aria-labelledby="bancos-conectados"
      className="mt-4 rounded border border-stone-200 p-3"
    >
      <h3 id="bancos-conectados" className="font-medium">
        Conexión con el banco (solo lectura)
      </h3>
      <p className="text-xs text-stone-600">
        <strong>Opción avanzada.</strong> No hace falta para llevar el banco: «Importar extractos
        del banco…» vale para cualquier banco y no pide ningún alta. La conexión es para quien
        quiera que los movimientos de BBVA o Revolut lleguen solos, y pide registrar una aplicación
        propia en Enable Banking (proveedor de información de cuentas autorizado según la normativa
        europea de pagos, PSD2). TaxPain sólo puede ver cuentas, saldos y movimientos: nada de esto
        permite mover dinero. Los movimientos entran como un extracto más, sin repetir los que ya
        estaban.
      </p>
      {msg && (
        <p role="alert" className="mt-1 text-sm text-red-700">
          {msg}
        </p>
      )}
      <div role="status">
        {lineas.length > 0 && (
          <ul className="mt-1 space-y-0.5 text-sm">
            {lineas.map((l) => (
              <li key={l.linea}>
                {l.linea}
                {l.avisos.length > 0 && (
                  <ul className="ml-4 list-disc text-xs">
                    {l.avisos.map((a) => (
                      <li
                        key={a}
                        className={avisoDestacado(a) ? 'text-amber-800' : 'text-stone-600'}
                      >
                        {a}
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      {listo ? (
        <>
          <Conexiones
            estado={estado}
            busy={busy}
            setBusy={setBusy}
            setEstado={setEstado}
            setMsg={setMsg}
            onDone={hecho}
          />
          <Conectar
            estado={estado}
            busy={busy}
            setBusy={setBusy}
            setMsg={setMsg}
            onDone={hecho}
            onCancel={load}
          />
        </>
      ) : null}
      <Configurar estado={estado} abierto={!listo} setEstado={setEstado} />
      <SinConexion />
    </section>
  );
}

function Conexiones({
  estado,
  busy,
  setBusy,
  setEstado,
  setMsg,
  onDone,
}: {
  estado: BancaEstado;
  busy: boolean;
  setBusy: (b: boolean) => void;
  setEstado: (e: BancaEstado) => void;
  setMsg: (m: string | null) => void;
  onDone: (a: BancaActualizacion) => void;
}) {
  const actualizar = async (conexion: number | null) => {
    setMsg(null);
    setBusy(true);
    try {
      onDone(await bancaActualizar(conexion));
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  if (estado.conexiones.length === 0) {
    return <p className="mt-2 text-sm text-stone-600">Todavía no hay ningún banco conectado.</p>;
  }
  const activas = estado.conexiones.some((c) => c.estado === 'activa');
  return (
    <div className="mt-2">
      <ul className="space-y-2">
        {estado.conexiones.map((c) => (
          <Conexion
            key={c.id}
            c={c}
            busy={busy}
            onActualizar={() => actualizar(c.id)}
            setEstado={setEstado}
            setMsg={setMsg}
          />
        ))}
      </ul>
      {activas && (
        <button
          type="button"
          className="mt-2 rounded-md btn-primary px-3 py-1 text-sm"
          disabled={busy}
          onClick={() => actualizar(null)}
        >
          {busy ? 'Descargando movimientos…' : 'Actualizar movimientos'}
        </button>
      )}
    </div>
  );
}

function Conexion({
  c,
  busy,
  onActualizar,
  setEstado,
  setMsg,
}: {
  c: ConexionBanco;
  busy: boolean;
  onActualizar: () => void;
  setEstado: (e: BancaEstado) => void;
  setMsg: (m: string | null) => void;
}) {
  const caducada = c.estado === 'caducada';
  const pronto = !caducada && c.dias !== null && c.dias < 15;
  return (
    <li className="rounded border border-stone-200 p-2 text-sm">
      <div className="flex flex-wrap items-baseline gap-x-3">
        <span className="font-medium">
          {c.banco} · {tipoDe(c.tipo)}
        </span>
        {caducada ? (
          <span className="text-amber-800">
            Permiso caducado o retirado: conecta otra vez este banco para renovarlo.
          </span>
        ) : (
          <span className={pronto ? 'text-amber-800' : 'text-stone-600'}>
            Permiso hasta el {dia(c.hasta)}
            {c.dias !== null && ` (${c.dias === 1 ? 'queda 1 día' : `quedan ${c.dias} días`})`}
            {pronto && ': renuévalo conectando otra vez este banco'}
          </span>
        )}
        {c.actualizada && (
          <span className="text-xs text-stone-500">Última descarga: {dia(c.actualizada)}</span>
        )}
      </div>
      {c.error && <p className="text-xs text-red-700">Última descarga: {c.error}</p>}
      <ul className="mt-1 space-y-0.5">
        {c.cuentas.map((k) => (
          <li key={k.id} className="flex flex-wrap items-baseline gap-x-3">
            <label>
              <input
                type="checkbox"
                checked={k.activa}
                disabled={busy}
                onChange={async (e) => {
                  setMsg(null);
                  try {
                    setEstado(await bancaActivarCuenta(k.id, e.target.checked));
                  } catch (err) {
                    setMsg(errorMessage(err));
                  }
                }}
              />{' '}
              {nombreCuenta(k)} ({k.divisa})
            </label>
            {k.saldo !== null && (
              <span className="text-xs text-stone-600">
                Saldo en el banco: {money(k.saldo, k.divisa)}
                {k.saldo_fecha && ` a ${dia(k.saldo_fecha)}`}
              </span>
            )}
            {k.subcuenta && <span className="text-xs text-stone-500">Subcuenta {k.subcuenta}</span>}
            {k.hasta_dia && (
              <span className="text-xs text-stone-500">Bajado hasta el {dia(k.hasta_dia)}</span>
            )}
            {k.tarjeta && !k.activa && (
              <span className="text-xs text-stone-500">
                Actívala si es de crédito y quieres conciliar sus compras.
              </span>
            )}
          </li>
        ))}
      </ul>
      <div className="mt-1 flex flex-wrap gap-3">
        {!caducada && (
          <button type="button" className="text-link" disabled={busy} onClick={onActualizar}>
            Actualizar {c.banco}
          </button>
        )}
        <ConfirmAction
          label={`Quitar la conexión con ${c.banco}`}
          question={`¿Quitar la conexión con ${c.banco}? Se retira el permiso en Enable Banking; los movimientos ya importados se quedan.`}
          confirmLabel="Quitar"
          disabled={busy}
          onConfirm={async () => {
            const r = await bancaDesconectar(c.id);
            setEstado(r.estado);
            if (r.aviso) setMsg(r.aviso);
          }}
        />
      </div>
    </li>
  );
}

function Conectar({
  estado,
  busy,
  setBusy,
  setMsg,
  onDone,
  onCancel,
}: {
  estado: BancaEstado;
  busy: boolean;
  setBusy: (b: boolean) => void;
  setMsg: (m: string | null) => void;
  onDone: (a: BancaActualizacion) => void;
  onCancel: () => void;
}) {
  const [bancos, setBancos] = useState<BancoDisponible[] | null>(null);
  const [esperando, setEsperando] = useState<string | null>(
    estado.pendiente ? `${estado.pendiente.banco} · ${tipoDe(estado.pendiente.tipo)}` : null,
  );
  // Con una autorización a medias (al volver a esta pantalla), la espera puede seguir en marcha.
  const [escucha, setEscucha] = useState(estado.pendiente !== null && estado.vuelta_local);
  const [direccion, setDireccion] = useState('');
  const pegar = useRef<HTMLInputElement>(null);
  // La conexión ya ha terminado (por la espera, por la dirección pegada) o se ha cancelado: un
  // error que llegue después por el otro camino ya no importa.
  const acabado = useRef(false);

  const elegir = async () => {
    setMsg(null);
    setBusy(true);
    try {
      setBancos(await bancaBancos());
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  const conectar = async (b: BancoDisponible, tipo: string) => {
    setMsg(null);
    setBusy(true);
    try {
      const r = await bancaConectar(b.name, b.country, tipo, b.maximum_consent_validity);
      acabado.current = false;
      setEsperando(`${b.name} · ${tipoDe(tipo)}`);
      setEscucha(r.escucha);
      setBancos(null);
      setBusy(false);
      setTimeout(() => pegar.current?.focus(), 0);
      if (r.escucha) {
        try {
          // `null`: se ha dejado de esperar (se ha pegado la dirección o se ha cancelado).
          const a = await bancaEsperar();
          if (a && !acabado.current) {
            acabado.current = true;
            setEsperando(null);
            onDone(a);
          }
        } catch (e) {
          if (!acabado.current) setMsg(errorMessage(e));
        }
      }
    } catch (e) {
      setMsg(errorMessage(e));
      setBusy(false);
    }
  };
  const terminar = async (ev: FormEvent) => {
    ev.preventDefault();
    setMsg(null);
    setBusy(true);
    try {
      const a = await bancaTerminar(direccion);
      acabado.current = true;
      setEsperando(null);
      setDireccion('');
      onDone(a);
    } catch (e) {
      if (!acabado.current) setMsg(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  if (esperando) {
    return (
      <form onSubmit={terminar} className="mt-3 space-y-2 text-sm">
        <p role="status">
          Autoriza el acceso de {esperando} en la página del banco que se ha abierto en el
          navegador.{' '}
          {escucha
            ? 'Cuando termines, TaxPain lo recogerá solo; si no, pega aquí la dirección.'
            : 'Al terminar, el navegador irá a una página que no carga: copia la dirección completa de la barra y pégala aquí.'}
        </p>
        <label className="flex flex-col">
          Dirección a la que te ha llevado el banco
          <input
            ref={pegar}
            className={input}
            value={direccion}
            onChange={(e) => setDireccion(e.target.value)}
            placeholder={`${estado.ajustes.url_vuelta}?state=…&code=…`}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <div className="flex gap-3">
          <button
            type="submit"
            className="rounded-md btn-primary px-3 py-1"
            disabled={busy || !direccion.trim()}
          >
            {busy ? 'Conectando…' : 'Terminar la conexión'}
          </button>
          <button
            type="button"
            className="text-link"
            onClick={async () => {
              acabado.current = true;
              setEsperando(null);
              setDireccion('');
              try {
                await bancaCancelar();
              } finally {
                onCancel();
              }
            }}
          >
            Cancelar
          </button>
        </div>
      </form>
    );
  }
  return (
    <div className="mt-3 text-sm">
      {bancos === null ? (
        <button type="button" className="text-link" disabled={busy} onClick={elegir}>
          Conectar un banco…
        </button>
      ) : (
        <fieldset className="space-y-1">
          <legend className="font-medium">¿Qué cuenta conectas?</legend>
          <div className="flex flex-wrap gap-2">
            {bancos.flatMap((b) =>
              (b.psu_types.length ? b.psu_types : ['business']).map((t) => (
                <button
                  key={`${b.name}-${b.country}-${t}`}
                  type="button"
                  className="rounded border px-2 py-1"
                  disabled={busy}
                  onClick={() => conectar(b, t)}
                >
                  {b.name} · {tipoDe(t)}
                  {b.country !== 'ES' ? ` (${b.country})` : ''}
                </button>
              )),
            )}
            <button type="button" className="text-link" onClick={() => setBancos(null)}>
              Cancelar
            </button>
          </div>
          <p className="text-xs text-stone-600">
            Se abre la página de tu banco para que autorices, con tu app o tu firma, el acceso de
            solo lectura (hasta 180 días; después se vuelve a autorizar).
          </p>
        </fieldset>
      )}
    </div>
  );
}

function Configurar({
  estado,
  abierto,
  setEstado,
}: {
  estado: BancaEstado;
  abierto: boolean;
  setEstado: (e: BancaEstado) => void;
}) {
  const [aceptado, setAceptado] = useState(estado.ajustes.aceptado);
  const [url, setUrl] = useState(estado.ajustes.url_vuelta);
  const [appId, setAppId] = useState(estado.app_id ?? '');
  const [msg, setMsg] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  const guardar = async (ev: FormEvent) => {
    ev.preventDefault();
    setMsg(null);
    setOk(null);
    try {
      setEstado(await bancaAjustes({ aceptado, url_vuelta: url }));
      setOk('Guardado.');
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  const clave = async () => {
    setMsg(null);
    setOk(null);
    const path = await pickPemFile();
    if (!path) return;
    // El navegador descarga la clave con el id de la aplicación como nombre.
    const nombre =
      path
        .split('/')
        .pop()
        ?.replace(/\.(pem|key)$/i, '') ?? '';
    const id = appId.trim() || (/^[0-9a-f-]{32,36}$/i.test(nombre) ? nombre : '');
    if (!id) {
      setMsg('Escribe primero el id de la aplicación (el que da Enable Banking al registrarla).');
      return;
    }
    try {
      const e = await bancaCredenciales(id, path);
      setAppId(e.app_id ?? id);
      setEstado(e);
      setOk(
        'Clave guardada, cifrada en tu Mac. Guarda el fichero .pem en un sitio seguro (no en Descargas): no va en las copias de seguridad y lo necesitarás para restaurar en otro Mac.',
      );
    } catch (e) {
      setMsg(errorMessage(e));
    }
  };
  return (
    <details
      open={abierto}
      className="mt-3 rounded border border-stone-200 bg-stone-50 p-3 text-sm"
    >
      <summary className="cursor-pointer font-medium">Configurar la conexión</summary>
      <ol className="mt-2 list-decimal space-y-1 pl-5 text-xs">
        <li>
          Crea una cuenta gratuita en{' '}
          <button type="button" className="text-link" onClick={() => openSede(PANEL_ENABLE)}>
            el panel de Enable Banking
          </button>{' '}
          (con tu email).
        </li>
        <li>
          Registra una aplicación de producción («Production») con esta dirección de vuelta:{' '}
          <code className="select-all">{url}</code>. Al registrarla, el navegador descarga su clave
          privada (un fichero .pem).
        </li>
        <li>
          Actívala vinculando tus propias cuentas de BBVA o Revolut, una a una: sólo se leen las
          cuentas vinculadas. Es el modo restringido de Enable Banking, sin coste, que sus
          condiciones reservan a pruebas y a uso personal. A las empresas se lo permiten como prueba
          y pueden retirarlo con aviso: para un uso estable hace falta un contrato con Enable
          Banking. Si se corta, sigues con los extractos en fichero y no pierdes nada.
        </li>
        <li>Guarda aquí el id de la aplicación y su fichero .pem, y acepta la conexión.</li>
      </ol>
      <div className="mt-3 max-w-2xl space-y-3">
        <label className="flex flex-col">
          Id de la aplicación
          <input
            className={input}
            value={appId}
            onChange={(e) => setAppId(e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <div className="flex flex-wrap items-baseline gap-3">
          <button type="button" className="rounded border px-2 py-1" onClick={clave}>
            {estado.clave ? 'Cambiar el fichero .pem…' : 'Elegir el fichero .pem…'}
          </button>
          {estado.clave && <span className="text-xs text-stone-600">Clave guardada.</span>}
          {estado.clave && (
            <ConfirmAction
              label="Borrar la clave"
              question="¿Borrar la clave de la aplicación? Las conexiones quedan, pero no se podrán usar hasta volver a guardarla."
              confirmLabel="Borrar"
              onConfirm={async () => {
                setEstado(await bancaQuitarCredenciales());
              }}
            />
          )}
        </div>
        <form onSubmit={guardar} className="space-y-3">
          <label className="flex flex-col">
            Dirección de vuelta (la misma que en la aplicación)
            <input
              className={input}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
            <span className="mt-1 text-xs text-stone-500">
              Con una dirección http://127.0.0.1:&lt;puerto&gt;/…, TaxPain recoge sola la vuelta del
              banco; con otra, se pega a mano.
            </span>
          </label>
          <label>
            <input
              type="checkbox"
              checked={aceptado}
              onChange={(e) => setAceptado(e.target.checked)}
            />{' '}
            <span>
              Acepto que TaxPain se conecte con Enable Banking para leer las cuentas que autorice en
              mi banco. Enable Banking ve los datos al pasarlos; TaxPain los guarda cifrados en este
              Mac y no los envía a nadie más.
            </span>
          </label>
          <button type="submit" className="rounded-md btn-primary px-3 py-1">
            Guardar
          </button>
        </form>
        {msg && (
          <p role="alert" className="text-red-700">
            {msg}
          </p>
        )}
        {ok && <p role="status">{ok}</p>}
      </div>
    </details>
  );
}

/** Lo habitual, sin conexión: cómo descargar los extractos de BBVA y Revolut para importarlos. */
function SinConexion() {
  return (
    <details className="mt-3 text-sm">
      <summary className="cursor-pointer">
        Lo habitual, sin conexión: descargar los extractos e importarlos
      </summary>
      <div className="mt-1 space-y-1 text-xs text-stone-700">
        <p>
          Descarga los movimientos de la banca online y pulsa «Importar extractos del banco…»: vale
          para cualquier banco y no se repiten los que ya estaban (tampoco los que hayan llegado por
          la conexión).
        </p>
        <ul className="list-disc space-y-1 pl-5">
          <li>
            <strong>BBVA</strong> (web o BBVA net cash): en los movimientos de la cuenta,
            descárgalos en Norma 43 (Cuaderno 43) si te lo ofrece, que trae los saldos y los códigos
            de cada movimiento; si no, en Excel. El PDF del extracto también sirve.
          </li>
          <li>
            <strong>Revolut</strong> (Business o particulares): en la cuenta, «Extracto»
            («Statement»), en Excel o CSV (o PDF), una vez por divisa si tienes varias.
          </li>
        </ul>
        <p>Los menús cambian a veces: busca «Descargar» o «Exportar» en los movimientos.</p>
      </div>
    </details>
  );
}
