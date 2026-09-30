#!/usr/bin/env bash
# La netlist del esquemático que escribe Riku (xschem-viewer-rust,
# `spice::netlist` en modo LVS) contra la de Xschem, con Netgen, en los
# esquemáticos de ejemplo de cada PDK. Uso (en el contenedor, con el ejemplo
# `spice` compilado):
#
#   cargo build --manifest-path external/xschem-viewer-rust/Cargo.toml --example spice
#   tools/verify/netlist/compare_xschem.sh [binario] [máximo por PDK]
#
# Un esquemático cuenta como equivalente si Netgen da el mismo veredicto
# comparando la nuestra con la de Xschem que la de Xschem consigo misma (hay
# testbench que Netgen no empareja ni así). El PDK se elige con
# `sak-pdk-script.sh`, como `sak-pdk`.
set -u
BIN=${1:-external/xschem-viewer-rust/target/debug/examples/spice}
MAX=${2:-60}
BIN=$(realpath "$BIN")
ROOT=${PDK_ROOT:-/foss/pdks}
OUT=$(mktemp -d)
status=0
# Conocidos: el bloque de código es un programa en Tcl (no hay intérprete).
known="sky130A/test_carry_lookahead"

for p in sky130A gf180mcuD ihp-sg13g2; do
  X=$ROOT/$p/libs.tech/xschem
  if [ ! -d "$X" ]; then echo "== $p: no instalado"; continue; fi
  # `sak-pdk-script.sh` usa variables que pueden no estar definidas.
  set +u
  # shellcheck disable=SC1091
  source sak-pdk-script.sh "$p" >/dev/null 2>&1
  set -u
  setup=$(ls "$ROOT/$p"/libs.tech/netgen/*setup.tcl 2>/dev/null | head -1)
  # El xschemrc de IHP no pone su propia carpeta en la ruta de símbolos.
  printf 'source %s/xschemrc\nappend XSCHEM_LIBRARY_PATH :%s\n' "$X" "$X" > "$OUT/rc_$p"
  n=0; ok=0; skip=0
  for sch in $(find "$X" -name '*.sch' | sort | head -"$MAX"); do
    name=$(basename "$sch" .sch); d=$OUT/$p/$name; mkdir -p "$d"
    (cd "$(dirname "$sch")" && timeout 30 xschem --rcfile "$OUT/rc_$p" --tcl "set lvs_netlist 1; set top_subckt 1" \
      -n -s -q -x --no_x -o "$d" -N ref.spice "$(basename "$sch")" >/dev/null 2>&1)
    if [ ! -s "$d/ref.spice" ] || grep -q "IS MISSING" "$d/ref.spice"; then skip=$((skip + 1)); continue; fi
    if ! (cd "$(dirname "$sch")" && timeout 30 "$BIN" --lvs --top "$sch" > "$d/ours.spice" 2> "$d/ours.err"); then
      echo "  $p/$name: error: $(head -c 200 "$d/ours.err")"; status=1; continue
    fi
    cp "$d/ref.spice" "$d/ref2.spice"
    (cd "$d" && timeout 60 netgen -batch lvs "ours.spice $name" "ref.spice $name" $setup comp.out >/dev/null 2>&1)
    (cd "$d" && timeout 60 netgen -batch lvs "ref2.spice $name" "ref.spice $name" $setup base.out >/dev/null 2>&1)
    base=$(grep -h "Final result" "$d/base.out" 2>/dev/null | tail -1)
    # Netgen no puede con la de Xschem sola: no hay con qué comparar.
    if [ -z "$base" ]; then skip=$((skip + 1)); continue; fi
    n=$((n + 1))
    ours=$(grep -h "Final result" "$d/comp.out" 2>/dev/null | tail -1)
    if [ "$ours" = "$base" ]; then
      ok=$((ok + 1))
    elif [[ " $known " == *" $p/$name "* ]]; then
      echo "  $p/$name: conocido (${ours:-sin resultado})"
    else
      echo "  $p/$name: ${ours:-sin resultado}  [Xschem consigo misma: $base]"; status=1
    fi
  done
  echo "== $p: $ok de $n equivalentes a Xschem ($skip sin referencia)"
done
echo "(netlists en $OUT)"
exit $status
