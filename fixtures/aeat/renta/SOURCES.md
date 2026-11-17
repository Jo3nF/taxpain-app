# Datos fiscales de Renta (modelo 100)

Descargados el 2026-10-14 del portal de desarrolladores de la AEAT
(`https://www.agenciatributaria.es/static_files/AEAT_Desarrolladores/EEDD/Modelo_100/2025/`;
el enlace de `sede.agenciatributaria.gob.es` del índice da 404).

- `Autenticacion-Datosfiscales-2025-EEDD.pdf`: «Instrucciones para la integración de la
  obtención de Datos Fiscales de Renta 2025 en programas de ayuda», v1.2 (15/07/2026).
  Ratificación del domicilio (`TOAG-JDIT/IsRatificadoExJson`), servicio
  `DFPA-D182/SvDesDF25Pei?nif=…&pdp=S|N` con el certificado del contribuyente, errores.
- `2025_Datos_Fiscales_EEDD.xlsx`: «Diseño de registro de datos fiscales y personales IRPF
  2025», v1.3. Registros de posiciones fijas (0 cabecera `DDFF2025`, 1 fecha de proceso, 2
  conceptos —RTC, PF, CT, RET…—, 9 cierre `99`); importes en céntimos, «N» inicial si negativos.
  TaxPain lee RTC (actividades económicas del 190), PF (pagos fraccionados 130/131), CT
  (cotizaciones) y RET (regularización del RETA): `crates/models/src/datos_fiscales.rs`.
f277fd1f9badd6bfffc9fa7eed23b90f2510f403a55a308748276de04827f4af  Autenticacion-Datosfiscales-2025-EEDD.pdf
76ac35c6a1bb5a610388eaeb5d16c9883db2b039c97321cf492486a1a786e5f4  2025_Datos_Fiscales_EEDD.xlsx
