#!/usr/bin/env sh
# Descarga riku de GitHub Releases, verifica el checksum e instala.
#
#   curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh
#       la última versión, en ~/.local/bin (sin sudo)
#   curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh -s -- v0.1.0
#       una versión concreta
#   curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sudo sh -s -- latest --system
#       en /usr/local/bin
#
# Lo que va después de la versión se le pasa a install.sh (--system).
set -eu

REPO=riku-chip/riku_chip
TAG=latest
if [ $# -gt 0 ]; then
    case "$1" in
        latest | v[0-9]*) TAG=$1; shift ;;
        [0-9]*) TAG=v$1; shift ;;
    esac
fi

say() { printf 'riku: %s\n' "$*" >&2; }
fail() { say "$*"; exit 1; }

[ "$(uname -s)" = Linux ] || fail "solo hay paquetes para Linux"
[ "$(uname -m)" = x86_64 ] || fail "solo hay paquetes para x86_64 (esta máquina es $(uname -m))"
for tool in curl tar sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || fail "falta $tool"
done

# La última: GitHub redirige releases/latest a releases/tag/<tag>.
if [ "$TAG" = latest ]; then
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
        || fail "no se pudo consultar la última versión"
    TAG=${url##*/}
    case "$TAG" in v[0-9]*) ;; *) fail "no hay releases publicados todavía" ;; esac
fi
VER=${TAG#v}
NAME=riku-$VER-linux-x86_64
BASE=https://github.com/$REPO/releases/download/$TAG

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM
cd "$TMP"

say "descargando $TAG"
curl -fsLO "$BASE/$NAME.tar.gz" || fail "no existe el release $TAG (ver https://github.com/$REPO/releases)"
curl -fsSLO "$BASE/SHA256SUMS" || fail "el release $TAG no tiene SHA256SUMS"
grep " $NAME.tar.gz\$" SHA256SUMS | sha256sum -c - >/dev/null || fail "el checksum no coincide: descarga dañada"
say "checksum OK"

tar xzf "$NAME.tar.gz"
sh "$NAME/install.sh" "$@"
