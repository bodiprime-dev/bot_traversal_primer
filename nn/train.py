"""Piolet-NN, étape 1 : le réseau prédit-il l'issue des parties mieux que notre évaluation ?

    python3 nn/train.py data.csv [--epochs 15] [--hidden 128] [--save weights.npz]

Données : `examples/dataset.rs` (une ligne par position, vue du camp au trait) :
    carte;résultat;recherche;statique;dense…;actives…

- Séparation par carte : les cartes 160 à 199 ne servent qu'au test (aucune fuite).
- Réseau : (600 binaires + 25 numériques) -> H (ReLU) -> 32 (ReLU) -> 1 (sigmoïde) = probabilité
  que le camp au trait gagne. Au plus 12 entrées binaires actives : la première couche est une
  somme de lignes de W1. Entraînement Adam, perte entropie croisée (nulle = 1/2).
- Références : l'évaluation statique et le score de la recherche, transformés en probabilité
  par une régression logistique ajustée sur les cartes d'entraînement.
"""

import argparse
import numpy as np

SPARSE, DENSE, TEST_FROM = 600, 25, 160


def load(path):
    boards, y, search, static, dense, idx = [], [], [], [], [], []
    with open(path) as f:
        for line in f:
            b, r, sc, st, d, s = line.rstrip("\n").split(";")
            boards.append(int(b))
            y.append((int(r) + 1) / 2)  # 1 gagné, 0,5 nul, 0 perdu
            search.append(float(sc))
            static.append(float(st))
            dense.append([float(v) for v in d.split(",")])
            a = [int(v) for v in s.split(",")]
            idx.append(a + [SPARSE] * (12 - len(a)))  # SPARSE : ligne nulle de remplissage
    return (np.array(boards), np.array(y), np.array(search), np.array(static),
            np.array(dense, dtype=np.float32), np.array(idx, dtype=np.int64))


def logloss(p, y):
    p = np.clip(p, 1e-6, 1 - 1e-6)
    return float(-np.mean(y * np.log(p) + (1 - y) * np.log(1 - p)))


def accuracy(p, y):
    decided = y != 0.5
    return float(np.mean((p[decided] > 0.5) == (y[decided] > 0.5)))


def fit_logistic(x, y, steps=3000, lr=0.5):
    """p = sigmoïde(a·x + b), x normalisé : une variable, descente de gradient."""
    m, s = x.mean(), x.std() + 1e-9
    z = (x - m) / s
    a, b = 1.0, 0.0
    for _ in range(steps):
        p = 1 / (1 + np.exp(-(a * z + b)))
        g = p - y
        a -= lr * np.mean(g * z)
        b -= lr * np.mean(g)
    return lambda v: 1 / (1 + np.exp(-(a * (v - m) / s + b)))


class Net:
    def __init__(self, hidden, rng):
        h2 = 32
        self.p = {
            "W1": rng.normal(0, 0.1, (SPARSE + 1, hidden)).astype(np.float32),
            "Wd": rng.normal(0, np.sqrt(2 / DENSE), (DENSE, hidden)).astype(np.float32),
            "b1": np.zeros(hidden, np.float32),
            "W2": rng.normal(0, np.sqrt(2 / hidden), (hidden, h2)).astype(np.float32),
            "b2": np.zeros(h2, np.float32),
            "W3": rng.normal(0, np.sqrt(1 / h2), (h2, 1)).astype(np.float32),
            "b3": np.zeros(1, np.float32),
        }
        self.p["W1"][SPARSE] = 0  # ligne de remplissage
        self.m = {k: np.zeros_like(v) for k, v in self.p.items()}
        self.v = {k: np.zeros_like(v) for k, v in self.p.items()}
        self.t = 0

    def forward(self, idx, dense):
        p = self.p
        a1 = p["W1"][idx].sum(1) + dense @ p["Wd"] + p["b1"]
        h1 = np.maximum(a1, 0)
        a2 = h1 @ p["W2"] + p["b2"]
        h2 = np.maximum(a2, 0)
        out = 1 / (1 + np.exp(-(h2 @ p["W3"] + p["b3"])[:, 0]))
        return out, (idx, dense, a1, h1, a2, h2)

    def step(self, idx, dense, y, lr, l2=1e-5):
        out, (idx, dense, a1, h1, a2, h2) = self.forward(idx, dense)
        n = len(y)
        g3 = ((out - y) / n)[:, None]  # dérivée de l'entropie croisée à travers la sigmoïde
        grads = {"W3": h2.T @ g3, "b3": g3.sum(0)}
        g2 = (g3 @ self.p["W3"].T) * (a2 > 0)
        grads["W2"], grads["b2"] = h1.T @ g2, g2.sum(0)
        g1 = (g2 @ self.p["W2"].T) * (a1 > 0)
        grads["Wd"], grads["b1"] = dense.T @ g1, g1.sum(0)
        gW1 = np.zeros_like(self.p["W1"])
        np.add.at(gW1, idx.ravel(), np.repeat(g1, idx.shape[1], axis=0))
        gW1[SPARSE] = 0
        grads["W1"] = gW1
        self.t += 1
        for k, g in grads.items():  # Adam
            g = g + l2 * self.p[k]
            self.m[k] = 0.9 * self.m[k] + 0.1 * g
            self.v[k] = 0.999 * self.v[k] + 0.001 * g * g
            mh = self.m[k] / (1 - 0.9 ** self.t)
            vh = self.v[k] / (1 - 0.999 ** self.t)
            self.p[k] -= lr * mh / (np.sqrt(vh) + 1e-8)

    def predict(self, idx, dense, batch=8192):
        return np.concatenate([self.forward(idx[i:i + batch], dense[i:i + batch])[0] for i in range(0, len(idx), batch)])


def report(name, p, y, ply_frac):
    phases = [("début", ply_frac < 0.1), ("milieu", (ply_frac >= 0.1) & (ply_frac < 0.25)), ("fin", ply_frac >= 0.25)]
    by = "  ".join(f"{n} {logloss(p[m], y[m]):.4f}/{accuracy(p[m], y[m]):.3f}" for n, m in phases if m.any())
    print(f"  {name:<22} log-loss {logloss(p, y):.4f}  précision {accuracy(p, y):.3f}   [{by}]")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("data")
    ap.add_argument("--epochs", type=int, default=15)
    ap.add_argument("--hidden", type=int, default=128)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--batch", type=int, default=512)
    ap.add_argument("--save")
    ap.add_argument("--test-from", type=int, default=TEST_FROM)
    a = ap.parse_args()

    boards, y, search, static, dense, idx = load(a.data)
    test = boards >= a.test_from
    train = ~test
    print(f"{len(y)} positions ({train.sum()} entraînement, {test.sum()} test sur les cartes {a.test_from}+)")
    ply_frac = dense[:, 22]  # demi-coup / limite

    rng = np.random.default_rng(1)
    net = Net(a.hidden, rng)
    tr = np.flatnonzero(train)
    for epoch in range(a.epochs):
        rng.shuffle(tr)
        for i in range(0, len(tr), a.batch):
            b = tr[i:i + a.batch]
            net.step(idx[b], dense[b], y[b], a.lr)
        p = net.predict(idx[test], dense[test])
        print(f"  époque {epoch + 1:>2} : test log-loss {logloss(p, y[test]):.4f}, précision {accuracy(p, y[test]):.3f}")

    print("\nSur les cartes de test (log-loss / précision ; détail par phase de jeu) :")
    report("constante (1/2)", np.full(test.sum(), 0.5), y[test], ply_frac[test])
    report("évaluation actuelle", fit_logistic(static[train], y[train])(static[test]), y[test], ply_frac[test])
    clip = lambda v: np.clip(v, -2000, 2000)  # victoires forcées : ±100 000
    report("recherche de Piolet", fit_logistic(clip(search[train]), y[train])(clip(search[test])), y[test], ply_frac[test])
    report("réseau", net.predict(idx[test], dense[test]), y[test], ply_frac[test])
    if a.save:
        np.savez(a.save, **net.p)
        print(f"poids enregistrés : {a.save}")


if __name__ == "__main__":
    main()
