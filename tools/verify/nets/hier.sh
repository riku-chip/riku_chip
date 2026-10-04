#!/usr/bin/env bash
# Extracción plana contra jerárquica (ronda 5): `hier_check` sobre layouts
# con jerarquía; cada uno tiene que dar "IGUALES". Uso (en el contenedor,
# con el ejemplo `hier_check` compilado en release):
#
#   tools/verify/nets/hier.sh [carpeta de demos]
#
# Sin carpeta usa solo la SRAM del repo. Con la de `riku demo --dir`, también
# el OTA, el inversor (Magic, sin meter nada en el padre) y tres partes de la
# macro del demo `chip` (la macro entera no se puede aplanar: ver
# `chip_vs_magic.sh`).
set -u
BIN=${HIER_BIN:-${CARGO_TARGET_DIR:-target}/release/examples/hier_check}
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
DEMOS=${1:-}
status=0
check() { # etiqueta, y lo que va a hier_check
  local name=$1
  shift
  local out
  out=$("$BIN" "$@" 2>&1)
  if grep -q "IGUALES" <<<"$out"; then
    echo "IGUALES  $name  ($(head -1 <<<"$out" | sed 's/.*jerárquica \([^ ]*\).*/jerárquica \1/'), $(grep plana <<<"$out" | sed 's/ *plana \([^:]*\):.*/plana \1/'))"
  else
    echo "DISTINTO $name"
    sed 's/^/    /' <<<"$out" | head -12
    status=1
  fi
}
check "sram_16x8 (examples/GDS)" "$ROOT/examples/GDS/sram_16x8_sky130.gds"
if [ -n "$DEMOS" ]; then
  check "ota" "$DEMOS/ota/layout/ota-5t.gds"
  RIKU_HIER_INLINE=0 check "inversor (sin meter nada)" "$DEMOS/inversor/layout/inv.mag"
  MACRO=$DEMOS/chip/sky130_sram_1kbyte_1rw1r_32x256_8.gds
  for c in port_data control_logic_rw port_address; do
    check "chip: $c" "$MACRO" "sky130_sram_1kbyte_1rw1r_32x256_8_$c"
  done
fi
exit $status
