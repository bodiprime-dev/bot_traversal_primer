"""Piolet-NN : exporte un réseau entraîné (.npz de train.py) vers le format lu par le bot.

    python3 nn/export.py poids.npz nn/weights.bin [--check data.csv]

Format (petit-boutiste) : b"PNN1", u32 SPARSE, DENSE, H1, H2, puis en f32 :
W1 ((SPARSE + 1) × H1, par ligne ; la dernière ligne est le remplissage, nulle), Wd (DENSE × H1),
b1 (H1), W2 (H1 × H2), b2 (H2), W3 (H2), b3 (1). Sortie : logit de la probabilité de gagner.

--check : écrit nn/check.csv (entrées de quelques positions et logit attendu) pour le test Rust
qui vérifie que l'inférence du bot redonne exactement les prédictions de numpy.
"""

import argparse
import struct
import numpy as np


def logit(p, idx, dense):
    a1 = p["W1"][idx].sum(1) + dense @ p["Wd"] + p["b1"]
    h2 = np.maximum(np.maximum(a1, 0) @ p["W2"] + p["b2"], 0)
    return (h2 @ p["W3"] + p["b3"])[:, 0]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("npz")
    ap.add_argument("out")
    ap.add_argument("--check")
    a = ap.parse_args()
    p = {k: v.astype(np.float32) for k, v in np.load(a.npz).items()}
    sparse, h1 = p["W1"].shape[0] - 1, p["W1"].shape[1]
    dense, h2 = p["Wd"].shape[0], p["W2"].shape[1]
    with open(a.out, "wb") as f:
        f.write(b"PNN1" + struct.pack("<4I", sparse, dense, h1, h2))
        for k in ["W1", "Wd", "b1", "W2", "b2", "W3", "b3"]:
            f.write(p[k].astype("<f4").tobytes())
    print(f"{a.out} : {sparse}+{dense} -> {h1} -> {h2} -> 1, {sum(v.size for v in p.values())} poids")
    if a.check:
        rows = []
        with open(a.check) as f:
            for line in f:
                rows.append(line.rstrip("\n").split(";"))
                if len(rows) == 8:
                    break
        out = []
        for r in rows:
            d = np.array([[float(v) for v in r[4].split(",")]], np.float32)
            s = [int(v) for v in r[5].split(",")]
            idx = np.array([s + [sparse] * (12 - len(s))])
            out.append(f"{r[4]};{r[5]};{logit(p, idx, d)[0]:.6f}")
        with open("nn/check.csv", "w") as f:
            f.write("\n".join(out) + "\n")
        print("nn/check.csv : 8 positions de référence")


if __name__ == "__main__":
    main()
