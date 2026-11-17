-- Método de amortización contable (lineal, porcentaje constante o suma de dígitos) y coeficiente
-- fiscal propio si el plan fiscal difiere del contable (puntos básicos; NULL = el mismo).
ALTER TABLE fixed_asset ADD COLUMN method TEXT NOT NULL DEFAULT 'lineal'
    CHECK (method IN ('lineal', 'constante', 'digitos'));
ALTER TABLE fixed_asset ADD COLUMN tax_rate_bp INTEGER
    CHECK (tax_rate_bp IS NULL OR (tax_rate_bp > 0 AND tax_rate_bp <= 10000));
