#!/bin/bash
# Uso: lanzar.sh <n> [limpiar]   -> pantalla :1<n>, puerto 44<n>0, datos en $E2E_DIR/area<n>/home
# Arranca (o reinicia) un Xvfb y un tauri-driver propios. Si se pasa «limpiar», borra los datos de la app.
n=$1; D=${E2E_DIR:-/tmp/taxpain-e2e}/area$n; mkdir -p $D/home $D/shots
for p in $(pgrep -f "tauri-driver --port 44${n}0"); do kill $p; done
for p in $(pgrep -f "WebKitWebDriver.*--port=44${n}1"); do kill $p; done
for p in $(pgrep -x taxpain-bin); do grep -qz "^HOME=$D/home$" /proc/$p/environ 2>/dev/null && kill $p; done
sleep 1
[ "$2" = limpiar ] && rm -rf $D/home/.local/share/es.taxpain.desktop
pgrep -f "Xvfb :1$n " >/dev/null || (Xvfb :1$n -screen 0 1400x900x24 >/dev/null 2>&1 &)
sleep 1
cd $D && DISPLAY=:1$n HOME=$D/home XDG_DATA_HOME=$D/home/.local/share XDG_CONFIG_HOME=$D/home/.config \
  nohup tauri-driver --port 44${n}0 --native-port 44${n}1 --native-driver /usr/bin/WebKitWebDriver > $D/driver.log 2>&1 &
sleep 2
echo "Área $n lista: WD_PORT=44${n}0 SHOTS=$D/shots, datos en $D/home/.local/share/es.taxpain.desktop"
