-- Forma de cobro o pago de cada vencimiento registrado en Cobros y pagos (las condiciones del
-- tercero sólo dan la forma por defecto). Hace falta para el 347: «importes percibidos en
-- metálico» superiores a 6.000 € por cliente y año (art. 34.1.h RGAT). NULL: no se dijo.
ALTER TABLE settlement ADD COLUMN method TEXT
    CHECK (method IS NULL OR method IN ('transferencia','recibo','efectivo','tarjeta','otro'));
