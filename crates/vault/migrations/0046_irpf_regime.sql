-- RETENCIONES-02: modalidad de estimación directa del IRPF (normal / simplificada) por ejercicio.
-- Los ejercicios nuevos se calculan en simplificada salvo que se diga otra cosa; los que ya tenían
-- algún 130 presentado antes de esta versión siguen como hasta ahora (sin el 5 % de gastos de
-- difícil justificación) y el borrador avisa para que el usuario elija. Aquí se apuntan esos
-- ejercicios (lista JSON de años) para no cambiarles nada en silencio.
INSERT OR IGNORE INTO settings(key, value)
SELECT 'models.irpf_regime_pending', COALESCE(json_group_array(year), '[]')
FROM (
  SELECT year FROM tax_return WHERE model = '130' AND environment = 'produccion'
  UNION
  SELECT year FROM filed_model WHERE model = '130'
  ORDER BY year
);
