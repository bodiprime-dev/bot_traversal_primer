#!/usr/bin/env bash
# Arène locale sans Docker : reproduit arena/Dockerfile du SDK en natif.
#
#   scripts/arena.sh setup                       # une fois : arène + 5 bots maison
#   scripts/arena.sh check                       # valide piolet.wasm (comme le site)
#   scripts/arena.sh duel montblanc --games 40   # duel, mêmes options que l'outil officiel
#
# Tout est construit dans .arena/ (ignoré par git), à partir du tag du SDK de Cargo.toml.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$ROOT/.arena
SDK_TAG=v0.2.0
ARENA=$WORK/sdk/arena/target/release/traversal-arena
WASM=$ROOT/target/wasm32-unknown-unknown/release/piolet.wasm
export HOUSE_DIR=$WORK/house

setup() {
  rustup target add wasm32-unknown-unknown
  mkdir -p "$WORK" "$HOUSE_DIR"
  [ -d "$WORK/sdk" ] || git clone --depth 1 --branch "$SDK_TAG" \
    https://github.com/Sylv-Lej/traversal-rust-sdk "$WORK/sdk"
  # Bots maison : le bot de départ compilé à chacun des 5 niveaux (galet … montblanc).
  for l in 1 2 3 4 5; do
    (cd "$WORK/sdk" && TRAVERSAL_LEVEL=$l cargo build --release --locked \
      --target wasm32-unknown-unknown -p traversal-starter-bot)
    cp "$WORK/sdk/target/wasm32-unknown-unknown/release/traversal_starter_bot.wasm" "$HOUSE_DIR/$l.wasm"
  done
  (cd "$WORK/sdk/arena" && cargo build --release --locked)
}

build() {
  (cd "$ROOT" && cargo build --release --target wasm32-unknown-unknown)
}

cmd=${1:-}
shift || true
case "$cmd" in
  setup) setup ;;
  check) build && "$ARENA" check "$WASM" "$@" ;;
  duel) build && "$ARENA" duel "$WASM" "$@" ;;
  *) echo "usage : $0 setup | check | duel <adversaire> [--games N] [--verbose]" >&2; exit 2 ;;
esac
