import { Plug } from 'lucide-react';
import { useEffect, useState } from 'react';
import { errorMessage, type McpAjustes, type McpEstado, mcpEstado, mcpGuardar } from './api';

/**
 * Acceso de Claude (Claude Desktop o Claude Code) a la empresa abierta por el servidor MCP
 * (`taxpain-mcp`). Apagado por defecto y empresa por empresa; lo que sale va sin anonimizar.
 */
export function ClaudeMcp() {
  const [estado, setEstado] = useState<McpEstado | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState('');
  useEffect(() => {
    mcpEstado()
      .then(setEstado)
      .catch((e) => setError(errorMessage(e)));
  }, []);

  const ajustes: McpAjustes = estado?.ajustes ?? { activado: false, nivel: 'consultar' };
  const guardar = async (a: McpAjustes) => {
    setBusy(true);
    setError(null);
    setSaved('');
    try {
      const e = await mcpGuardar(a);
      setEstado(e);
      setSaved(
        e.ajustes.activado
          ? e.ajustes.nivel === 'borradores'
            ? 'Activado: Claude puede consultar esta empresa y dejar borradores.'
            : 'Activado: Claude puede consultar esta empresa.'
          : 'Desactivado: Claude ya no ve esta empresa.',
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  const ruta = estado?.binario ?? '/ruta/a/taxpain-mcp';
  const config = JSON.stringify({ mcpServers: { taxpain: { command: ruta } } }, null, 2);

  return (
    <section className="mt-8" aria-labelledby="claude-mcp-title">
      <div className="flex items-start gap-3">
        <span className="icon-tile" aria-hidden="true">
          <Plug size={18} />
        </span>
        <div>
          <h2 id="claude-mcp-title" className="text-lg font-semibold">
            Claude (MCP)
          </h2>
          <p className="text-sm text-stone-600">
            Deja que Claude (Claude Desktop o Claude Code) consulte esta empresa desde tu Mac. Verá
            lo que le haga falta para responderte: facturas, clientes y proveedores con su NIF,
            movimientos del banco con IBAN y concepto, el libro mayor y los modelos.
          </p>
          <p className="mt-2 text-sm text-stone-600">
            <strong>Va sin anonimizar</strong> (el asistente de TaxPain sustituye nombres y NIF;
            esto no) y sale de tu Mac hacia el proveedor del modelo que uses con Claude. No puede
            emitir, presentar, firmar ni borrar nada, ni usar tu certificado.
          </p>
        </div>
      </div>
      <div className="card mt-4 space-y-4 p-5 text-sm">
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            role="switch"
            aria-checked={ajustes.activado}
            disabled={estado === null || busy}
            checked={ajustes.activado}
            onChange={(e) => void guardar({ ...ajustes, activado: e.target.checked })}
          />
          <span>
            Dejar que Claude consulte esta empresa
            <span className="block text-stone-500">
              Se activa empresa por empresa. Las que no lo tienen activado, Claude no las ve.
            </span>
          </span>
        </label>
        <fieldset disabled={estado === null || busy || !ajustes.activado} className="space-y-2">
          <legend className="font-medium">Hasta dónde llega</legend>
          <label className="flex items-start gap-2">
            <input
              type="radio"
              name="mcp-nivel"
              checked={ajustes.nivel === 'consultar'}
              onChange={() => void guardar({ ...ajustes, nivel: 'consultar' })}
            />
            <span>Sólo consultar</span>
          </label>
          <label className="flex items-start gap-2">
            <input
              type="radio"
              name="mcp-nivel"
              checked={ajustes.nivel === 'borradores'}
              onChange={() => void guardar({ ...ajustes, nivel: 'borradores' })}
            />
            <span>
              Consultar y dejar borradores
              <span className="block text-stone-500">
                Un presupuesto o una proforma en borrador y facturas recibidas pendientes de
                revisar. Nada se emite ni se contabiliza sin ti.
              </span>
            </span>
          </label>
        </fieldset>
        <p role="status" className="text-stone-600">
          {saved}
        </p>
        {error && (
          <p role="alert" className="text-red-700">
            {error}
          </p>
        )}
        <p className="text-stone-600">
          Cada consulta de Claude queda en la auditoría de la empresa (qué herramienta, no lo que
          devuelve). Para cortarlo, apaga el interruptor: vale desde la siguiente consulta.
        </p>
        <details>
          <summary className="cursor-pointer font-medium">Cómo conectarlo</summary>
          <div className="mt-2 space-y-2 text-stone-700">
            {!estado?.binario && (
              <p>
                El programa <code>taxpain-mcp</code> aún no viene con la app: compílalo como explica{' '}
                <code>docs/CLAUDE_CODE.md</code> («Servidor MCP») y pon su ruta abajo.
              </p>
            )}
            <p>
              En Claude Desktop: Ajustes → Desarrollador → Editar configuración, y añade a{' '}
              <code>claude_desktop_config.json</code>:
            </p>
            <pre className="whitespace-pre-wrap break-all rounded-md bg-stone-100 p-3 text-xs">
              {config}
            </pre>
            <p>En Claude Code, en la Terminal:</p>
            <pre className="whitespace-pre-wrap break-all rounded-md bg-stone-100 p-3 text-xs">
              {`claude mcp add taxpain -- "${ruta}"`}
            </pre>
            <p>
              Los textos de facturas, movimientos y contactos los escriben terceros: Claude los
              recibe como datos, pero revisa siempre lo que te proponga hacer con ellos.
            </p>
          </div>
        </details>
      </div>
    </section>
  );
}
