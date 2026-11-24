#!/bin/sh
# Install the optional Citation Style Language (CSL) data for Pandoc 3.13.
set -eu

usage() {
    printf '%s\n' 'Usage: fetch-csl-locales.sh [DATA_DIR]' \
        'The default is $DATA_DIR or the Pandoc user data directory.'
}

case "${1:-}" in
    -h|--help) usage; exit 0 ;;
    -*) usage >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then
    usage >&2
    exit 2
fi

data_dir=${1:-${DATA_DIR:-}}
if [ -z "$data_dir" ]; then
    if [ -n "${XDG_DATA_HOME:-}" ]; then
        data_dir=$XDG_DATA_HOME/pandoc
    elif [ -n "${APPDATA:-}" ]; then
        data_dir=$APPDATA/pandoc
    else
        data_dir=${HOME:?Set HOME or supply DATA_DIR}/.local/share/pandoc
        if [ ! -d "$data_dir" ] && [ -d "$HOME/.pandoc" ]; then
            data_dir=$HOME/.pandoc
        fi
    fi
fi
case "$data_dir" in /*|[A-Za-z]:/*) ;; *) data_dir=./$data_dir ;; esac

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)

check_folder() {
    folder=$1
    while [ ! -d "$folder" ]; do
        folder=$(dirname -- "$folder")
    done
    folder=$(CDPATH= cd -- "$folder" && pwd -P)
    case "$folder/" in
        "$repo/"*) printf '%s\n' 'Select a data directory outside the Randoc repository.' >&2; exit 2 ;;
    esac
    if command -v git >/dev/null 2>&1 && git -C "$folder" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        printf '%s\n' 'Select a data directory outside Git worktrees.' >&2
        exit 2
    fi
}

check_folder "$data_dir"
check_folder "$data_dir/csl/locales"
mkdir -p "$data_dir/csl/locales"
locale_dir=$data_dir/csl/locales
check_folder "$data_dir"
check_folder "$locale_dir"

printf '%s\n' \
    'These optional files use the Creative Commons Attribution-ShareAlike 3.0 licence.' \
    'https://creativecommons.org/licenses/by-sa/3.0/' \
    'Style source: Pandoc 3.13, data/default.csl (Chicago author-date).' \
    'Locale source: citeproc 0.14.1.1, from the Citation Style Language project.'

check_folder "${TMPDIR:-/tmp}"
stage=$(mktemp -d "${TMPDIR:-/tmp}/randoc-csl.XXXXXXXX")
trap 'rm -rf "$stage"' 0
trap 'exit 1' HUP INT TERM

verify() {
    expected=$1
    if command -v sha256sum >/dev/null 2>&1; then
        digest=$(sha256sum < "$2")
    else
        digest=$(shasum -a 256 < "$2")
    fi
    if [ "${digest%% *}" != "$expected" ]; then
        printf 'SHA-256 mismatch for %s\n' "$2" >&2
        exit 1
    fi
}

curl -fsSL -o "$stage/default.csl" \
    https://raw.githubusercontent.com/jgm/pandoc/3.13/data/default.csl
verify 42a41c813009fcf5b3450345fda4d67464738a2379f1bb90f65f9cc267502075 "$stage/default.csl"
curl -fsSL -o "$stage/citeproc.tar.gz" \
    https://hackage-content.haskell.org/package/citeproc-0.14.1.1/citeproc-0.14.1.1.tar.gz
verify e5a353f975c32169979b4c9e158d3edb9e57a5f653fab8ff0505a2067e2b1b4b "$stage/citeproc.tar.gz"
tar -tzf "$stage/citeproc.tar.gz" > "$stage/archive-members"
awk '/^citeproc-0\.14\.1\.1\/locales\/[A-Za-z0-9-]+\.xml$/ {print}' \
    "$stage/archive-members" > "$stage/locales"
if [ "$(wc -l < "$stage/locales" | tr -d ' ')" -ne 60 ]; then
    printf '%s\n' 'Expected 60 locale files in the citeproc archive.' >&2
    exit 1
fi
tar -xzf "$stage/citeproc.tar.gz" -C "$stage" -T "$stage/locales"

check_destination() {
    if [ -L "$2" ] || { [ -e "$2" ] && { [ ! -f "$2" ] || ! cmp -s "$1" "$2"; }; }; then
        printf 'Move the existing file before installation: %s\n' "$2" >&2
        exit 1
    fi
}

check_destination "$stage/default.csl" "$data_dir/default.csl"
while IFS= read -r member; do
    check_destination "$stage/$member" "$locale_dir/locales-${member##*/}"
done < "$stage/locales"
cp "$stage/default.csl" "$data_dir/default.csl"
while IFS= read -r member; do
    cp "$stage/$member" "$locale_dir/locales-${member##*/}"
done < "$stage/locales"
printf 'Installed default.csl and 60 locale files in %s\n' "$data_dir"
