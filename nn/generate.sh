#!/bin/bash
# Génération reprenable des données de Piolet-NN : lots de LOT parties (250 par défaut), un
# fichier par lot ; BIN choisit le générateur (dataset, deep…).
# Un lot terminé reçoit un marqueur .done ; relancer le script reprend au premier lot manquant.
#   nn/generate.sh <dossier> <première partie> <dernière partie (exclue)> [nœuds] [graine]
set -euo pipefail
DIR=$1; FROM=$2; TO=$3; NODES=${4:-3000}; SEED=${5:-11}; LOT=${LOT:-250}
BIN=${BIN:-$(dirname "$0")/../target/release/examples/dataset}  # BIN : binaire figé, pour des lots homogènes
mkdir -p "$DIR"
for ((g = FROM; g < TO; g += LOT)); do
  n=$(( TO - g < LOT ? TO - g : LOT ))
  f="$DIR/lot_$(printf %05d $g)"
  [ -e "$f.done" ] && continue
  "$BIN" --first $g --games $n --nodes $NODES --seed $SEED > "$f.csv" 2> "$f.log"
  touch "$f.done"
done
echo FINI
