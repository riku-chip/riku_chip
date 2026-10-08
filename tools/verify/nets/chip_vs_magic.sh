#!/usr/bin/env bash
# Un chip entero contra Magic (ronda 5): la macro de 1 KB del demo `chip`
# extraída por celdas con Riku (`hier_check`, que escribe su SPICE) y con
# Magic (`gds read` + `extract all` + `ext2spice lvs`), comparadas con
# Netgen. Tarda: Magic unos 40 min y Netgen unos 20 en el contenedor. Uso:
#
#   tools/verify/nets/chip_vs_magic.sh <carpeta del demo chip> [trabajo]
#
# Resultado del 2026-10-04: la topología coincide salvo en el bitcell de
# doble puerto de OpenRAM (`sky130_fd_bd_sram__openram_dp_cell`), donde Riku
# y Magic ya difieren con la extracción plana de esa sola celda (ver
# `docs/dev/roadmap.md`).
set -u
DEMO=$1
WORK=${2:-$(mktemp -d)}
BIN=${HIER_BIN:-${CARGO_TARGET_DIR:-target}/release/examples/hier_check}
ROOT=${PDK_ROOT:-/foss/pdks}
export PATH=/foss/tools/bin:$PATH PDK_ROOT=$ROOT
GDS=$DEMO/sky130_sram_1kbyte_1rw1r_32x256_8.gds
TOP=sky130_sram_1kbyte_1rw1r_32x256_8
mkdir -p "$WORK"
cd "$WORK" || exit 1
if [ ! -f macro.spice ]; then
  cat > x.tcl <<TCL
gds flatglob *_contact_*
gds read $GDS
load $TOP
select top cell
extract all
ext2spice lvs
ext2spice -o $WORK/macro.spice
quit -noprompt
TCL
  magic -dnull -noconsole -rcfile "$ROOT/sky130A/libs.tech/magic/sky130A.magicrc" x.tcl > magic.log 2>&1
fi
HIER_NO_FLAT=1 HIER_SPICE=$WORK/riku.spice "$BIN" "$GDS"
netgen -batch lvs "riku.spice $TOP" "macro.spice $TOP" "$ROOT/sky130A/libs.tech/netgen/sky130A_setup.tcl" lvs.out > netgen.log 2>&1
grep -E "^Final result|Device classes .* are equivalent" lvs.out | tail -3
echo "detalle: $WORK/lvs.out"
