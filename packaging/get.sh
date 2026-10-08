#!/usr/bin/env sh
# Downloads riku from GitHub Releases, verifies the checksum and installs it.
#
#   curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh
#       the latest release, into ~/.local/bin (no sudo)
#   curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh -s -- v0.1.0
#       a specific release
#   curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sudo sh -s -- latest --system
#       into /usr/local/bin
#
# Anything after the version is passed on to install.sh (--system).
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

[ "$(uname -s)" = Linux ] || fail "packages are only available for Linux"
[ "$(uname -m)" = x86_64 ] || fail "packages are only available for x86_64 (this machine is $(uname -m))"
for tool in curl tar sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool is required"
done

# Latest: GitHub redirects releases/latest to releases/tag/<tag>.
if [ "$TAG" = latest ]; then
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
        || fail "could not look up the latest release"
    TAG=${url##*/}
    case "$TAG" in v[0-9]*) ;; *) fail "no releases have been published yet" ;; esac
fi
VER=${TAG#v}
NAME=riku-$VER-linux-x86_64
BASE=https://github.com/$REPO/releases/download/$TAG

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM
cd "$TMP"

say "downloading $TAG"
curl -fsLO "$BASE/$NAME.tar.gz" || fail "release $TAG does not exist (see https://github.com/$REPO/releases)"
curl -fsSLO "$BASE/SHA256SUMS" || fail "release $TAG has no SHA256SUMS"
grep " $NAME.tar.gz\$" SHA256SUMS | sha256sum -c - >/dev/null || fail "checksum mismatch: the download is corrupted"
say "checksum OK"

tar xzf "$NAME.tar.gz"
sh "$NAME/install.sh" "$@"
