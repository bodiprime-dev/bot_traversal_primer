#!/usr/bin/env python3
"""Comparaison appariée de deux réglages, cumulée sur plusieurs échantillons de parties.

    python3 scripts/abpool.py ref1.list:new1.list ref2.list:new2.list …

Chaque fichier est la sortie de `duel --list` (une ligne par partie : numéro, résultat pour Piolet,
motif). Les parties de même numéro dans `ref` et `new` partagent carte, couleur et ouverture
(`--open N --oseed S`) : on compare leurs scores un à un. Sortie : scores moyens, écart moyen par
partie, erreur standard (écart-type des différences / racine de n), z.
"""
import sys

import numpy as np


def scores(path):
    out = {}
    for line in open(path):
        p = line.split()
        out[int(p[0])] = (int(p[1]) + 1) / 2  # 1 victoire, 1/2 nulle, 0 défaite
    return out


def main():
    ref_all, new_all, per_pair = [], [], []
    for spec in sys.argv[1:]:
        ref_path, new_path = spec.split(":")
        r, n = scores(ref_path), scores(new_path)
        common = sorted(set(r) & set(n))
        ref_all += [r[g] for g in common]
        new_all += [n[g] for g in common]
        d = np.array([n[g] - r[g] for g in common])
        per_pair.append((len(common), 100 * np.mean([r[g] for g in common]), 100 * np.mean([n[g] for g in common]), 100 * d.mean()))
    ref, new = np.array(ref_all), np.array(new_all)
    d = new - ref
    se = d.std(ddof=1) / np.sqrt(len(d))
    for i, (n, a, b, diff) in enumerate(per_pair):
        print(f"  échantillon {i + 1}: {n} parties, référence {a:.1f} %, variante {b:.1f} %, écart {diff:+.1f}")
    print(f"cumul : {len(d)} parties appariées — référence {100 * ref.mean():.1f} %, variante {100 * new.mean():.1f} %, "
          f"écart {100 * d.mean():+.2f} points ± {200 * se:.2f} (IC 95 %), z = {d.mean() / se:+.2f}")


if __name__ == "__main__":
    main()
