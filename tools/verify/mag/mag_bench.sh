#!/usr/bin/env bash
# Medición de la fase 8 (Magic): diff de una jerarquía real con las
# sub-celdas en el mismo commit.
#   c1: la librería sky130_fd_io completa (mag/)
#   c2: un README (el .mag de arriba no cambia)
#   c3: un rect de metal1 nuevo en una sub-celda
#   c4: la misma sub-celda con un rect partido en dos tiras (sin cambio real)
set -eu
RIKU=${RIKU:-/headless/riku-target/ws/release/riku}
SRC=/foss/pdks/sky130A/libs.ref/sky130_fd_io/mag
TOP=sky130_fd_io__top_gpio_ovtv2
R=/tmp/magbench; rm -rf $R; mkdir -p $R/mag; cd $R; git init -q
g() { git -c user.name=t -c user.email=t@t "$@"; }
cp $SRC/*.mag mag/; g add .; g commit -qm c1
echo x > README; g add README; g commit -qm c2
# Sub-celda usada por el top (primer `use` que exista en la carpeta).
SUB=$(awk '/^use /{print $2}' mag/$TOP.mag | while read c; do [ -f mag/$c.mag ] && echo $c && break; done)
echo "top: $TOP ($(ls mag | wc -l) archivos), sub-celda editada: $SUB"
sed -i 's/^<< metal1 >>$/<< metal1 >>\nrect 0 0 400 400/' mag/$SUB.mag
grep -q "rect 0 0 400 400" mag/$SUB.mag || { printf '<< metal1 >>\nrect 0 0 400 400\n' >> mag/$SUB.mag; }
g commit -qam c3
# c4: partir el rect nuevo en dos tiras (misma geometría).
sed -i 's/^rect 0 0 400 400$/rect 0 0 200 400\nrect 200 0 400 400/' mag/$SUB.mag; g commit -qam c4

t() { # etiqueta, comando
  local s=$(date +%s.%N)
  /usr/bin/time -f "%M" -o /tmp/mem.txt bash -c "$2" >/tmp/out.txt 2>&1 || true
  printf '%-44s %6.2f s  %5d MB  %s\n' "$1" "$(echo "$(date +%s.%N) - $s" | bc)" $(( $(cat /tmp/mem.txt) / 1024 )) "$(grep -c '^  [+~-]' /tmp/out.txt) cambios"
}
export RIKU_NO_CACHE=1
t "diff top c1→c2 (sin cambios)"          "$RIKU diff HEAD~3 HEAD~2 mag/$TOP.mag"
t "diff top c2→c3 (rect en $SUB)"         "$RIKU diff HEAD~2 HEAD~1 mag/$TOP.mag"
t "diff top c3→c4 (mismas figuras)"       "$RIKU diff HEAD~1 HEAD mag/$TOP.mag"
t "diff sub-celda c2→c3"                  "$RIKU diff HEAD~2 HEAD~1 mag/$SUB.mag"
t "log (4 commits, ~1000 .mag)"           "$RIKU log"
echo "--- salida de c2→c3:"; $RIKU diff HEAD~2 HEAD~1 mag/$TOP.mag | head -12
