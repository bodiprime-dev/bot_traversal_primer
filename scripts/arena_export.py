#!/usr/bin/env python3
"""Export de parties du site (JSON) -> format texte lu par `examples/arena_games.rs`.

    python3 scripts/arena_export.py data/arena/export.json > games.txt

Une ligne par partie : id|camp de Piolet|résultat|raison|adversaire|mode du terrain|hauteurs|coups
Coups : de-vers-type séparés par des espaces (type m, a, x pour une sortie, p pour une passe).
"""
import json
import sys

KIND = {"move": "m", "attack": "a", "exit": "x", "pass": "p"}
d = json.load(open(sys.argv[1]))
for g in d["games"]:
    opp = g["blue"] if g["side"] == 1 else g["red"]
    moves = " ".join(f"{m['from'] if m['from'] is not None else 255}-{m['to'] if m['to'] is not None else 255}-{KIND[m['kind']]}"
                     for m in g["moves"])
    heights = ",".join(map(str, g["terrain"]["heights"]))
    print("|".join([g["id"], str(g["side"]), g["result"], g["reason"], opp["name"].replace("|", "/"),
                    g["terrain"]["mode"], heights, moves]))
