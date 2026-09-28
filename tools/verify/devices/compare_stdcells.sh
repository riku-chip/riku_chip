#!/usr/bin/env bash
# Transistores de Riku contra la netlist de referencia de las celdas
# estándar de cada PDK instalado. Uso (en el contenedor, con el ejemplo
# `devices` compilado en release):
#
#   tools/verify/devices/compare_stdcells.sh [binario de devices]
set -u
BIN=${1:-${CARGO_TARGET_DIR:-target}/release/examples/devices}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=${PDK_ROOT:-/foss/pdks}
status=0
check() { # nombre layout netlist [escala de la netlist] [celdas conocidas]
  # layout: un .gds (todas sus celdas) o una carpeta de .mag (cada una).
  local name=$1 gds=$2 net=$3 scale=${4:-1} known=${5:-}
  if [ ! -e "$gds" ] || [ ! -f "$net" ]; then echo "== $name: no instalado"; return; fi
  echo "== $name"
  local out; out=$(mktemp)
  local cells=""
  if [ -f "$gds" ]; then cells=$(python3 - "$gds" <<'PY'
import sys, struct
# Nombres de todas las celdas del GDS (registros STRNAME).
data = open(sys.argv[1], 'rb').read(); i = 0; names = []
while i + 4 <= len(data):
    n, t = struct.unpack('>HB', data[i:i+3])
    if n < 4: break
    if t == 0x06: names.append(data[i+4:i+n].rstrip(b'\0').decode('latin-1'))
    i += n
print(' '.join(names))
PY
); fi
  # shellcheck disable=SC2086
  "$BIN" "$gds" $cells > "$out" 2>/dev/null || { echo "  devices falló"; status=1; return; }
  python3 "$HERE/compare_spice.py" "$out" "$net" --scale "$scale" --known "$known" || status=1
  rm -f "$out"
}
check sky130_fd_sc_hd "$ROOT/sky130A/libs.ref/sky130_fd_sc_hd/gds/sky130_fd_sc_hd.gds" "$ROOT/sky130A/libs.ref/sky130_fd_sc_hd/spice/sky130_fd_sc_hd.spice" 1e-6
# Conocidas: la netlist del PDK no coincide con su layout; Riku da lo mismo
# que KLayout (clkbuf_1: una compuerta de W 1,05, no dos de 0,525;
# dfrbp_1: tres transistores más anchos que en la netlist).
check gf180mcu_fd_sc_mcu7t5v0 "$ROOT/gf180mcuD/libs.ref/gf180mcu_fd_sc_mcu7t5v0/gds/gf180mcu_fd_sc_mcu7t5v0.gds" "$ROOT/gf180mcuD/libs.ref/gf180mcu_fd_sc_mcu7t5v0/spice/gf180mcu_fd_sc_mcu7t5v0.spice" 1 gf180mcu_fd_sc_mcu7t5v0__clkbuf_1
check sg13g2_stdcell "$ROOT/ihp-sg13g2/libs.ref/sg13g2_stdcell/gds/sg13g2_stdcell.gds" "$ROOT/ihp-sg13g2/libs.ref/sg13g2_stdcell/spice/sg13g2_stdcell.spice" 1 sg13g2_dfrbp_1
# Las mismas celdas en Magic (IHP no las trae en .mag).
check "sky130_fd_sc_hd (.mag)" "$ROOT/sky130A/libs.ref/sky130_fd_sc_hd/mag" "$ROOT/sky130A/libs.ref/sky130_fd_sc_hd/spice/sky130_fd_sc_hd.spice" 1e-6
check "gf180mcu_fd_sc_mcu7t5v0 (.mag)" "$ROOT/gf180mcuD/libs.ref/gf180mcu_fd_sc_mcu7t5v0/mag" "$ROOT/gf180mcuD/libs.ref/gf180mcu_fd_sc_mcu7t5v0/spice/gf180mcu_fd_sc_mcu7t5v0.spice" 1 gf180mcu_fd_sc_mcu7t5v0__clkbuf_1
exit $status
