"""Piolet-NN : apprendre à choisir le coup, sur des positions profondes et intéressantes.

    python3 nn/rank.py <lots…> [--init poids.npz] [--epochs 20] [--save poids.npz]

Données : `examples/deep.rs` (positions critiques ou difficiles, 8 coups candidats chacune, le
premier étant le meilleur coup d'une recherche profonde). Pour chaque candidat : la position
après le coup, vue de l'adversaire, son score profond et notre évaluation actuelle.

Le réseau f évalue chaque position après coup ; pour nous, le coup j vaut s_j = −f(enfant_j).
- Perte de classement : entropie croisée entre softmax(s) et la cible softmax(−score / T) (les
  scores profonds, vus du camp qui joue) : un coup presque aussi bon que le meilleur n'est pas une
  faute. Coups élagués par le faisceau : très mauvais.
- Perte de valeur (poids --value) : f(enfant) ≈ sigmoïde(score profond / ÉCHELLE), pour que les
  valeurs restent calibrées (la recherche compare des valeurs, pas seulement des rangs).

Mesure (cartes 160+) : part des positions où chaque méthode met le meilleur coup en tête parmi
les 8 candidats (hasard : 1/8), sur toutes les positions et sur les difficiles (là où une
recherche de 3 000 nœuds s'était trompée).
"""

import argparse
import glob
import numpy as np

from train import Net, SPARSE, DENSE

UNSEARCHED = -1_000_000_000
SCALE = 270.0  # points d'évaluation par unité de logit (étape 2)


def load(paths):
    boards, hard, scores, static, dense, idx = [], [], [], [], [], []
    for path in paths:
        with open(path) as f:
            for line in f:
                parts = line.rstrip("\n").split(";")
                if len(parts) != 5:
                    continue  # ligne tronquée (lot interrompu)
                cands = parts[4].split(" ")
                if len(cands) != 8:
                    continue
                sc, st, de, ix = [], [], [], []
                for c in cands:
                    s, v, d, a = c.split("|")
                    sc.append(int(s))
                    st.append(int(v))
                    de.append([float(x) for x in d.split(",")])
                    a = [int(x) for x in a.split(",")] if a else []
                    ix.append(a + [SPARSE] * (12 - len(a)))
                boards.append(int(parts[0]))
                hard.append(int(parts[2]))
                scores.append(sc)
                static.append(st)
                dense.append(de)
                idx.append(ix)
    return (np.array(boards), np.array(hard, bool), np.array(scores, np.float64), np.array(static, np.float64),
            np.array(dense, np.float32), np.array(idx, np.int64))


def child_scores(scores):
    """Scores des coups vus du camp qui joue : −(score de l'enfant vu de l'adversaire)."""
    s = -scores
    s[scores == UNSEARCHED] = -5000.0  # coup élagué : très mauvais
    return np.clip(s, -5000, 5000)


def softmax(x):
    e = np.exp(x - x.max(1, keepdims=True))
    return e / e.sum(1, keepdims=True)


def top1(values):
    """Part des positions où le candidat 0 (le meilleur coup profond) a la plus grande valeur."""
    return float(np.mean(np.argmax(values, 1) == 0))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("lots", nargs="+")
    ap.add_argument("--init", help="poids de départ (.npz, par ex. le réseau de l'étape 1)")
    ap.add_argument("--hidden", type=int, default=32)
    ap.add_argument("--epochs", type=int, default=20)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--l2", type=float, default=1e-3)
    ap.add_argument("--temp", type=float, default=60.0, help="température T de la cible (points)")
    ap.add_argument("--value", type=float, default=0.5, help="poids de la perte de valeur")
    ap.add_argument("--batch", type=int, default=128, help="positions par lot (× 8 candidats)")
    ap.add_argument("--test-from", type=int, default=160)
    ap.add_argument("--save")
    a = ap.parse_args()

    paths = [p for g in a.lots for p in sorted(glob.glob(g))]
    boards, hard, scores, static, dense, idx = load(paths)
    n, k = scores.shape
    test = boards >= a.test_from
    val = ~test & (boards >= a.test_from - 20)
    train = ~test & ~val
    print(f"{n} positions ({train.sum()} entraînement, {val.sum()} validation, {test.sum()} test), {hard.sum()} difficiles")

    mine = child_scores(scores)  # (n, 8), vu du camp qui joue
    target = softmax(mine / a.temp)
    # f(enfant) ≈ probabilité que le camp qui joue dans l'enfant gagne ≈ σ(score de l'enfant / ÉCHELLE).
    value_target = 1 / (1 + np.exp(-scores.clip(-5000, 5000) / SCALE))
    searched = scores != UNSEARCHED

    rng = np.random.default_rng(1)
    net = Net(a.hidden, rng)
    if a.init:
        init = np.load(a.init)
        if init["W2"].shape[0] == a.hidden:
            net.p = {key: init[key].astype(np.float32).copy() for key in net.p}
            print(f"départ : {a.init}")

    def values(sel):
        """s_j = −logit(f(enfant_j)) pour les positions `sel`."""
        flat_idx = idx[sel].reshape(-1, 12)
        flat_dense = dense[sel].reshape(-1, DENSE)
        out = net.predict(flat_idx, flat_dense)
        z = np.log(np.clip(out, 1e-7, 1 - 1e-7) / np.clip(1 - out, 1e-7, 1))
        return -z.reshape(-1, k)

    def report(title):
        print(f"\n{title} (cartes {a.test_from}+ ; meilleur coup en tête parmi 8 : toutes / difficiles)")
        for name, v in [("hasard", rng.random((test.sum(), k))), ("évaluation actuelle", -static[test]), ("réseau", values(test))]:
            print(f"  {name:<20} {top1(v):.3f} / {top1(v[hard[test]]):.3f}")

    report("Avant entraînement")
    best, best_p = None, None
    tr = np.flatnonzero(train)
    for epoch in range(a.epochs):
        rng.shuffle(tr)
        for i in range(0, len(tr), a.batch):
            b = tr[i:i + a.batch]
            out, cache = net.forward(idx[b].reshape(-1, 12), dense[b].reshape(-1, DENSE))
            out = np.clip(out, 1e-7, 1 - 1e-7)
            z = np.log(out / (1 - out)).reshape(-1, k)
            # Classement : s = −z ; d(CE)/ds = softmax(s) − cible ; d/dz = −(…).
            d_rank = -(softmax(-z) - target[b]) / len(b)
            # Valeur : entropie croisée sur les candidats évalués ; d/dz = (f − cible).
            m = searched[b]
            d_val = a.value * (out.reshape(-1, k) - value_target[b]) * m / max(1, m.sum())
            net.backward(cache, (d_rank + d_val).ravel(), a.lr, a.l2)
        v = values(val)
        score = top1(v)
        if best is None or score > best:
            best, best_p = score, {key: x.copy() for key, x in net.p.items()}
        print(f"  époque {epoch + 1:>2} : validation {score:.3f}")
    net.p = best_p
    report("Après entraînement (meilleure époque en validation)")
    if a.save:
        np.savez(a.save, **net.p)
        print(f"poids enregistrés : {a.save}")


if __name__ == "__main__":
    main()
