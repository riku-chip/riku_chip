#!/usr/bin/env bash
# Compara lo que lee Riku con lo que lee KLayout.
#
#   tools/verify/compare.sh                       # librerias de celdas SKY130, GF180 e IHP
#   tools/verify/compare.sh lib.gds [otra.oas]    # librerias propias
#   tools/verify/compare.sh --xor a.gds b.gds CELDA
#
# Sale con codigo 1 si alguna comparacion difiere. Ver README.md.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
PDK_ROOT="${PDK_ROOT:-/foss/pdks}"
OUT="${OUT:-${TMPDIR:-/tmp}/riku-verify}"
mkdir -p "$OUT"

echo "Compilando verify_dump..." >&2
(cd "$ROOT/gds-renderer" && cargo build -q --release --example verify_dump)
DUMP="${CARGO_TARGET_DIR:-$ROOT/gds-renderer/target}/release/examples/verify_dump"

status=0
compare() { # nombre, archivo riku, archivo klayout
    if diff -q "$2" "$3" >/dev/null; then
        printf '%-40s idéntico (%s líneas)\n' "$1" "$(wc -l <"$2")"
    else
        printf '%-40s %s diferencias -> diff %s %s\n' "$1" "$(diff "$2" "$3" | grep -c '^[<>]')" "$2" "$3"
        status=1
    fi
}

if [[ "${1:-}" == "--xor" ]]; then
    [[ $# -eq 4 ]] || { echo "uso: $0 --xor a b celda" >&2; exit 2; }
    "$DUMP" xor "$2" "$3" "$4" >"$OUT/xor.riku"
    python3 "$HERE/klayout_dump.py" xor "$2" "$3" "$4" >"$OUT/xor.klayout"
    compare "XOR $4" "$OUT/xor.riku" "$OUT/xor.klayout"
    exit $status
fi

libs=("$@")
if [[ ${#libs[@]} -eq 0 ]]; then
    libs=(
        "$PDK_ROOT/sky130A/libs.ref/sky130_fd_sc_hd/gds/sky130_fd_sc_hd.gds"
        "$PDK_ROOT/gf180mcuD/libs.ref/gf180mcu_fd_sc_mcu7t5v0/gds/gf180mcu_fd_sc_mcu7t5v0.gds"
        "$PDK_ROOT/ihp-sg13g2/libs.ref/sg13g2_stdcell/gds/sg13g2_stdcell.gds"
    )
fi
for lib in "${libs[@]}"; do
    name="$(basename "$lib")"
    "$DUMP" cells "$lib" >"$OUT/$name.riku"
    python3 "$HERE/klayout_dump.py" cells "$lib" >"$OUT/$name.klayout"
    compare "$name ($(grep -c '^CELL' "$OUT/$name.riku") celdas)" "$OUT/$name.riku" "$OUT/$name.klayout"
done
exit $status
