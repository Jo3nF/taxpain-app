#!/usr/bin/env bash
# Genera .p12 SINTÉTICOS (RSA) para tests. Nunca uses datos reales.
# NIF de prueba válidos por algoritmo: 12345678Z (titular), B12345674 (entidad).
# Contraseña de todos: Prueba-1234. Requiere OpenSSL 3 (RC2-40 usa el proveedor legacy).
set -euo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
PASS=Prueba-1234

openssl req -x509 -newkey rsa:2048 -nodes -days 7300 -keyout "$work/ca.key" -out "$work/ca.pem" \
  -subj "/C=ES/O=FNMT-RCM PRUEBAS/OU=Ceres/CN=AC FNMT Usuarios PRUEBAS" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null

client() { # nombre sujeto
  openssl req -newkey rsa:2048 -nodes -keyout "$work/$1.key" -out "$work/$1.csr" -subj "$2" \
    -utf8 2>/dev/null
  printf 'keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=clientAuth,emailProtection\n' > "$work/ext"
  openssl x509 -req -in "$work/$1.csr" -CA "$work/ca.pem" -CAkey "$work/ca.key" -CAcreateserial \
    -days 7000 -out "$work/$1.pem" -extfile "$work/ext" 2>/dev/null
}

client pf "/C=ES/serialNumber=IDCES-12345678Z/GN=CARMEN/SN=ESPAÑOLA ESPAÑOLA/CN=ESPAÑOLA ESPAÑOLA CARMEN - 12345678Z"
client rep "/C=ES/O=EMPRESA DE PRUEBA SL/organizationIdentifier=VATES-B12345674/serialNumber=IDCES-12345678Z/CN=12345678Z CARMEN ESPAÑOLA (R: B12345674)"

openssl pkcs12 -export -in "$work/pf.pem" -inkey "$work/pf.key" -certfile "$work/ca.pem" \
  -passout "pass:$PASS" -out rsa-persona-fisica-aes.p12
openssl pkcs12 -export -in "$work/rep.pem" -inkey "$work/rep.key" -certfile "$work/ca.pem" \
  -passout "pass:$PASS" -out rsa-representante-3des.p12 \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
openssl pkcs12 -export -legacy -in "$work/pf.pem" -inkey "$work/pf.key" -certfile "$work/ca.pem" \
  -passout "pass:$PASS" -out rsa-persona-fisica-rc2.p12
# Clave RSA sintética de una «aplicación» de Enable Banking para los tests de crates/banca (no
# hace falta regenerarla con los .p12). Va en DER para que no haya claves en PEM en el
# repositorio: el test le pone las cabeceras en memoria.
[ -f rsa-app-banca.der ] || openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -outform DER -out rsa-app-banca.der
echo "OK: $(ls *.p12 | tr '\n' ' ')"
