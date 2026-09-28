#!/usr/bin/env bash
# Segundo oráculo: la netlist que extrae Magic de unas celdas de un GDS,
# contra la de Riku, con Netgen. Sirve para desempatar las celdas cuya
# netlist de referencia del PDK no coincide con Riku: si Magic da lo mismo
# que Riku, la que no refleja el layout es la referencia. Uso (en el
# contenedor, con el ejemplo `nets` compilado en release):
#
#   tools/verify/nets/magic_vs_riku.sh <pdk> <layout.gds> <celda…>
#
# <pdk>: sky130A, gf180mcuD o ihp-sg13g2.
set -u
PDK=$1
GDS=$2
shift 2
BIN=${NETS_BIN:-${CARGO_TARGET_DIR:-target}/release/examples/nets}
ROOT=${PDK_ROOT:-/foss/pdks}
export PATH=/foss/tools/bin:$PATH PDK_ROOT=$ROOT
RC=$ROOT/$PDK/libs.tech/magic/$PDK.magicrc
SETUP=$ROOT/$PDK/libs.tech/netgen/${PDK}_setup.tcl
# SKY130: W y L sin sufijo en Riku; Magic escribe `.option scale` (ver abajo).
UNIT=u
[ "$PDK" = sky130A ] && UNIT=""
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
status=0
for cell in "$@"; do
  d=$work/$cell
  mkdir -p "$d"
  cat > "$d/x.tcl" <<EOF
gds read $GDS
load $cell
select top cell
extract all
ext2spice lvs
ext2spice -o $d/magic.spice
quit -noprompt
EOF
  (cd "$d" && timeout 300 magic -dnull -noconsole -rcfile "$RC" x.tcl > magic.log 2>&1)
  "$BIN" --unit "$UNIT" "$GDS" "$cell" > "$d/riku.spice" 2>/dev/null
  netgen -batch lvs "$d/riku.spice $cell" "$d/magic.spice $cell" "$SETUP" "$d/lvs.out" > /dev/null 2>&1
  result=$(grep -E "^Final result" -A1 "$d/lvs.out" | tail -1)
  [ -z "$result" ] && result=$(grep -m1 "Final result" "$d/lvs.out")
  if grep -q "Circuits match uniquely" "$d/lvs.out" && ! grep -q "Property errors were found" "$d/lvs.out"; then
    echo "IGUAL a Magic  $cell"
  else
    echo "DIFIERE de Magic  $cell: $(grep -m1 -E 'Final result|do not match|failed' "$d/lvs.out")"
    status=1
  fi
done
exit $status
