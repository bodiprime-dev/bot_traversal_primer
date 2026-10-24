"""Piolet-NN : le réseau fait-il mieux que l'évaluation sur les comparaisons fiables du test ?

    python3 nn/bootstrap.py <lots.csv> <poids.npz> [--hidden 32]

Rééchantillonne les POSITIONS du test (les paires d'une même position sont corrélées) et donne
l'intervalle de confiance à 95 % du gain de précision du réseau (en correction de l'évaluation,
comme `rank.py --residual`) sur l'évaluation actuelle.
"""
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rank
from train import Net
data, npz = sys.argv[1], sys.argv[2]
b, hard, sc, st, de, ix = rank.load([data])
ok, w = rank.pairs(sc, 20.0)
test = b >= 160
rows = np.flatnonzero(test); k = 8
params = {key: v.astype(np.float32) for key, v in np.load(npz).items()}
net = Net(params["W2"].shape[0], np.random.default_rng(0))
net.p = params
zs = []
for i in range(0, len(rows), 4096):
    r = rows[i:i+4096]; _, cache = net.forward(ix[r].reshape(-1, 12), de[r].reshape(-1, 25))
    zs.append(((cache[5] @ net.p["W3"] + net.p["b3"])[:, 0]).reshape(-1, k) + st[r] / rank.SCALE)
z_net = np.concatenate(zs); z_eval = st[rows] / rank.SCALE; o = ok[rows]
def per_pos(z):
    d = z[:, 1:] - z[:, :1]; win = (d > 1e-9) * 1.0 + (np.abs(d) <= 1e-9) * 0.5
    return (win * o).sum(1), o.sum(1)
(wn, n), (we, _) = per_pos(z_net), per_pos(z_eval)
rng = np.random.default_rng(0); diffs = []
for _ in range(5000):
    s = rng.integers(0, len(n), len(n)); diffs.append((wn[s].sum() - we[s].sum()) / n[s].sum())
lo, hi = np.percentile(diffs, [2.5, 97.5])
print(f"{len(n)} positions, {int(n.sum())} paires : évaluation {we.sum()/n.sum():.3f}, réseau {wn.sum()/n.sum():.3f}, gain {np.mean(diffs):+.3f}  (IC 95 % par position : {lo:+.3f} à {hi:+.3f}) ; réseau meilleur dans {np.mean(np.array(diffs)>0):.1%} des tirages")
