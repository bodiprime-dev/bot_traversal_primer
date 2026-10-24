#!/usr/bin/env python3
"""Échanges de Sentinelles dans des parties `duel --list`.

    python3 scripts/exchanges.py a.list [b.list …]

Un échange : au demi-coup p, un camp capture une Sentinelle ; au demi-coup p + 1, l'autre camp
capture la pièce qui vient de la prendre (même type). Sans les cases, c'est une approximation
(une capture d'une autre pièce du même type compterait aussi), mais elle suffit à comparer deux
réglages. Compte : échanges où Piolet a perdu sa Sentinelle d'abord (il a été pris puis a repris),
et où il a pris d'abord (puis a été repris) ; score des parties concernées.
"""
import re
import sys

for path in sys.argv[1:]:
    taken_first = given_first = games = 0
    scores_ex, n_ex = 0.0, 0
    for line in open(path):
        p = line.split()
        games += 1
        m = re.search(r"prises:(\S*)", line)
        caps = []
        for c in (m.group(1).split(",") if m and m.group(1) else []):
            who, rest = c.split(":")
            att, tgt = rest.split("@")[0].split("x")
            caps.append((who, att, tgt, int(rest.split("@")[1])))
        had = False
        for (w1, a1, t1, p1), (w2, a2, t2, p2) in zip(caps, caps[1:]):
            if t1 == "S" and p2 == p1 + 1 and w2 != w1 and t2 == a1:
                had = True
                if w1 == "M":
                    given_first += 1  # Mont Blanc prend notre Sentinelle, on reprend
                else:
                    taken_first += 1  # on prend leur Sentinelle, ils reprennent
        if had:
            n_ex += 1
            scores_ex += (int(p[1]) + 1) / 2
    print(f"{path.split('/')[-1]:<16} {games} parties : échanges où Piolet prend d'abord {taken_first}, "
          f"où il est pris d'abord {given_first} ; parties avec échange {n_ex} (score {100 * scores_ex / max(1, n_ex):.0f} %)")
