#!/usr/bin/env bash
# Compara polígonos y área por capa de jerarquías Magic (.mag) leídas por
# gdstk-rs (ejemplo mag_area) y por KLayout (klayout_mag_area.py), aplanadas
# y sin unir: la misma cantidad de polígonos y la misma suma de áreas.
#
#   tools/verify/mag/compare_mag.sh                     # casos de los PDK (SKY130 y GF180)
#   tools/verify/mag/compare_mag.sh top.mag LAMBDA [DIR...]
#
# Diferencias esperadas, que no cuentan: capas que Riku deja fuera
# (checkpaint, error_*, ...) y que KLayout lee como capas comunes.
# Sale con código 1 si alguna capa difiere. Ver docs/dev/development.md, "Verification".
set -euo pipefail
export LC_ALL=C

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
PDK_ROOT="${PDK_ROOT:-/foss/pdks}"
OUT="${OUT:-${TMPDIR:-/tmp}/riku-verify-mag}"
# KLayout 0.30.4 y anteriores ignoran magscale: hace falta 0.30.12 o más nuevo
# (p. ej. python3 -m venv /tmp/kl && /tmp/kl/bin/pip install klayout==0.30.12).
KLAYOUT_PY="${KLAYOUT_PY:-python3}"
mkdir -p "$OUT"

echo "Compilando mag_area..." >&2
(cd "$ROOT/external/gdstk/rust" && cargo build -q --release --example mag_area)
AREA="${CARGO_TARGET_DIR:-$ROOT/external/gdstk/rust/target}/release/examples/mag_area"

HINT='^(space|checkpaint|CP|checksubcell|CS|error_p|EP|error_s|ES|error_ps|EPS|magnet|fence|rotate)\b'
status=0

compare() { # top lambda dirs...
    local top="$1" lam="$2"; shift 2
    local name; name="$(basename "$top" .mag)"
    local libargs=()
    for d in "$@"; do libargs+=(--lib "$d"); done
    "$AREA" "$top" --lambda "$lam" "${libargs[@]}" >"$OUT/$name.riku" 2>"$OUT/$name.riku.err"
    "$KLAYOUT_PY" "$HERE/klayout_mag_area.py" "$top" "$lam" "$@" 2>"$OUT/$name.klayout.err" \
        | grep -Ev "$HINT" >"$OUT/$name.klayout" || true
    # Misma cantidad de polígonos; área igual salvo el redondeo de la salida.
    local bad
    bad=$(join -t $'\t' -a1 -a2 -e MISSING -o 0,1.2,2.2,1.3,2.3 "$OUT/$name.riku" "$OUT/$name.klayout" \
        | awk -F'\t' '$2=="MISSING" || $3=="MISSING" || $2!=$3 || ($4-$5)^2 > 1e-10 + 1e-18*$4*$4 {print}')
    if [[ -z "$bad" ]]; then
        printf '%-48s idéntico (%s capas)\n' "$name" "$(wc -l <"$OUT/$name.riku")"
    else
        printf '%-48s difiere:\n%s\n' "$name" "$bad"
        status=1
    fi
}

if [[ $# -ge 2 ]]; then
    compare "$@"
    exit $status
fi

S=$PDK_ROOT/sky130A/libs.ref
G=$PDK_ROOT/gf180mcuD/libs.ref
sky_libs=("$S"/*/mag)
gf_libs=("$G"/*/mag)
compare "$S/sky130_fd_io/mag/sky130_fd_io__top_gpio_ovtv2.mag" 0.01 "${sky_libs[@]}"
compare "$S/sky130_fd_io/mag/sky130_fd_io__top_hvclampv2.mag" 0.01 "${sky_libs[@]}"
compare "$S/sky130_fd_sc_hd/mag/sky130_fd_sc_hd__inv_1.mag" 0.01 "${sky_libs[@]}"
compare "$S/sky130_fd_sc_hd/mag/sky130_fd_sc_hd__dfxtp_1.mag" 0.01 "${sky_libs[@]}"
compare "$S/sky130_fd_pr/mag/sky130_fd_pr__cap_vpp_08p6x07p8_m1m2_noshield.mag" 0.01 "${sky_libs[@]}"
compare "$G/gf180mcu_fd_ip_sram/mag/gf180mcu_fd_ip_sram__sram64x8m8wm1.mag" 0.05 "${gf_libs[@]}"
compare "$G/gf180mcu_fd_io/mag/gf180mcu_fd_io__bi_t.mag" 0.05 "${gf_libs[@]}"
compare "$G/gf180mcu_fd_sc_mcu7t5v0/mag/gf180mcu_fd_sc_mcu7t5v0__inv_1.mag" 0.05 "${gf_libs[@]}"
exit $status
