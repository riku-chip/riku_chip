#!/usr/bin/env bash
# Redes de Riku contra la netlist de referencia de las celdas estándar de
# cada PDK instalado, con Netgen. Uso (en el contenedor, con el ejemplo
# `nets` compilado en release):
#
#   tools/verify/nets/compare_stdcells.sh [binario de nets]
set -u
BIN=${1:-${CARGO_TARGET_DIR:-target}/release/examples/nets}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=${PDK_ROOT:-/foss/pdks}
export PATH=/foss/tools/bin:$PATH
status=0
check() { # nombre layout netlist setup unidad [celdas conocidas]
  # layout: un .gds (todas sus celdas) o una carpeta de .mag (cada una).
  local name=$1 layout=$2 net=$3 setup=$4 unit=$5 known=${6:-}
  if [ ! -e "$layout" ] || [ ! -f "$net" ]; then echo "== $name: no instalado"; return; fi
  echo "== $name"
  local out; out=$(mktemp)
  local cells=""
  if [ -f "$layout" ]; then cells=$(python3 "$HERE/gds_cells.py" "$layout"); fi
  # shellcheck disable=SC2086
  "$BIN" --unit "$unit" "$layout" $cells > "$out" 2>/dev/null || { echo "  nets falló"; status=1; return; }
  python3 "$HERE/compare_netgen.py" "$out" "$net" "$setup" --known "$known" || status=1
  rm -f "$out"
}
S=$ROOT/sky130A/libs.ref/sky130_fd_sc_hd
G=$ROOT/gf180mcuD/libs.ref/gf180mcu_fd_sc_mcu7t5v0
I=$ROOT/ihp-sg13g2/libs.ref/sg13g2_stdcell
# Conocidas, cada una comparada también con la extracción de Magic
# (magic_vs_riku.sh):
# - probe_p_8 y probec_p_8: su layout tiene un resistor de metal5 hasta el
#   pin X que la netlist del PDK no trae (Riku y Magic sí);
# - la netlist del PDK no coincide con su layout y Riku da lo mismo que
#   Magic: dfbbp_1, lsbuf_lh_isowell_4 y xor3_4 de SKY130 (pines que el GDS
#   no dibuja), y a22oi_1, dfrbp_1, dfrbp_2, slgcp_1 y tiehi de IHP.
K=sky130_fd_sc_hd__
SKY_KNOWN=${K}probe_p_8,${K}probec_p_8,${K}dfbbp_1,${K}lpflow_lsbuf_lh_isowell_4,${K}xor3_4
IHP_KNOWN=sg13g2_a22oi_1,sg13g2_dfrbp_1,sg13g2_dfrbp_2,sg13g2_slgcp_1,sg13g2_tiehi
# SKY130: W y L sin sufijo (su netlist usa `.option scale=1e-6`).
check sky130_fd_sc_hd "$S/gds/sky130_fd_sc_hd.gds" "$S/spice/sky130_fd_sc_hd.spice" "$ROOT/sky130A/libs.tech/netgen/sky130A_setup.tcl" "" "$SKY_KNOWN"
check "sky130_fd_sc_hd (.mag)" "$S/mag" "$S/spice/sky130_fd_sc_hd.spice" "$ROOT/sky130A/libs.tech/netgen/sky130A_setup.tcl" "" "$SKY_KNOWN"
check gf180mcu_fd_sc_mcu7t5v0 "$G/gds/gf180mcu_fd_sc_mcu7t5v0.gds" "$G/spice/gf180mcu_fd_sc_mcu7t5v0.spice" "$ROOT/gf180mcuD/libs.tech/netgen/gf180mcuD_setup.tcl" u
check "gf180mcu_fd_sc_mcu7t5v0 (.mag)" "$G/mag" "$G/spice/gf180mcu_fd_sc_mcu7t5v0.spice" "$ROOT/gf180mcuD/libs.tech/netgen/gf180mcuD_setup.tcl" u
check sg13g2_stdcell "$I/gds/sg13g2_stdcell.gds" "$I/spice/sg13g2_stdcell.spice" "$ROOT/ihp-sg13g2/libs.tech/netgen/ihp-sg13g2_setup.tcl" u "$IHP_KNOWN"
exit $status
