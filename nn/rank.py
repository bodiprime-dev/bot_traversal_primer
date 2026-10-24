"""Piolet-NN : apprendre à choisir le coup, sur des positions profondes et intéressantes.

    python3 nn/rank.py <lots…> [--residual] [--epochs 20] [--save poids.npz]

Données : `examples/deep.rs` (positions critiques ou difficiles, 8 coups candidats chacune, le
premier étant le meilleur coup d'une recherche profonde). Pour chaque candidat : la position
après le coup, vue de l'adversaire, son score profond et notre évaluation actuelle.

Seules les comparaisons FIABLES servent. La recherche examine les coups autres que le premier avec
une fenêtre nulle : leur « score » est une borne (« pas mieux que le meilleur »), souvent égale
au meilleur score sans être une égalité. Le meilleur coup est exact ; un autre coup dont la borne
est au moins `--margin` points sous lui est vraiment moins bon. Les autres comparaisons (bornes
égales ou proches, coups élagués) ne disent rien et sont ignorées.

Perte par paires : pour chaque coup j nettement moins bon que le meilleur coup 0,
    log(1 + exp(z_0 − z_j))      (z = logit que le camp qui joue dans l'enfant gagne)
c'est-à-dire : l'enfant du meilleur coup doit être moins bon pour l'adversaire que celui du coup j.
Pondération par la certitude (écart / 60, entre 0,3 et 1).

--residual : z = évaluation actuelle / ÉCHELLE + réseau (le réseau apprend une correction, initialisée
à zéro : on part exactement du niveau actuel).

Mesure (cartes de test) : part des comparaisons fiables où le meilleur coup est classé au-dessus du
coup moins bon, pour l'évaluation actuelle et le réseau, sur toutes les positions et sur les
difficiles (là où une recherche de 3 000 nœuds s'était trompée). Hasard : 0,5.
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


def pairs(scores, margin):
    """Comparaisons fiables (n, 7) : le coup j+1 est nettement moins bon que le meilleur. Poids
    de certitude (0 si la comparaison est ignorée)."""
    mine = np.where(scores == UNSEARCHED, np.nan, -scores)  # score vu du camp qui joue
    gap = mine[:, :1] - mine[:, 1:]  # > 0 : le meilleur coup est devant ; NaN : coup élagué
    ok = np.nan_to_num(gap, nan=-1.0) >= margin
    return ok, np.where(ok, np.clip(np.nan_to_num(gap) / 60.0, 0.3, 1.0), 0.0)


def accuracy(z, ok, rows=None):
    """Part des paires fiables où z_0 < z_j (ex æquo : 1/2). z : (n, 8), logit des enfants."""
    if rows is not None:
        z, ok = z[rows], ok[rows]
    d = z[:, 1:] - z[:, :1]
    win = (d > 1e-9) * 1.0 + (np.abs(d) <= 1e-9) * 0.5
    return float((win * ok).sum() / max(1, ok.sum()))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("lots", nargs="+")
    ap.add_argument("--residual", action="store_true")
    ap.add_argument("--hidden", type=int, default=32)
    ap.add_argument("--epochs", type=int, default=20)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--l2", type=float, default=1e-3)
    ap.add_argument("--margin", type=float, default=20.0, help="écart minimal (points) pour une paire fiable")
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
    ok, weight = pairs(scores, a.margin)
    print(f"{n} positions ({train.sum()} entraînement, {val.sum()} validation, {test.sum()} test), {hard.sum()} difficiles")
    print(f"paires fiables (marge {a.margin:g}) : entraînement {ok[train].sum()}, validation {ok[val].sum()}, test {ok[test].sum()}")

    rng = np.random.default_rng(1)
    net = Net(a.hidden, rng)
    base = static / SCALE if a.residual else np.zeros_like(static)  # évaluation actuelle de l'enfant, en logit
    if a.residual:
        net.p["W3"][:] = 0  # la correction part de zéro : on démarre exactement sur l'évaluation actuelle
        net.p["b3"][:] = 0

    def raw(cache):
        return (cache[5] @ net.p["W3"] + net.p["b3"])[:, 0]  # logit du réseau seul

    def logits(rows, batch=4096):
        zs = []
        for i in range(0, len(rows), batch):
            b = rows[i:i + batch]
            _, cache = net.forward(idx[b].reshape(-1, 12), dense[b].reshape(-1, DENSE))
            zs.append(raw(cache).reshape(-1, k) + base[b])
        return np.concatenate(zs)

    def report(title):
        rows = np.flatnonzero(test)
        h = hard[rows]
        print(f"\n{title} — comparaisons fiables du test : meilleur coup classé au-dessus (toutes / difficiles)")
        z_eval = static[rows] / SCALE
        z_net = logits(rows)
        for name, z in [("évaluation actuelle", z_eval), ("réseau", z_net)]:
            print(f"  {name:<20} {accuracy(z, ok[rows]):.3f} / {accuracy(z, ok[rows], h):.3f}")

    report("Avant entraînement")
    best, best_p = -1.0, {key: x.copy() for key, x in net.p.items()}
    tr = np.flatnonzero(train)
    vrows = np.flatnonzero(val)
    for epoch in range(a.epochs):
        rng.shuffle(tr)
        for i in range(0, len(tr), a.batch):
            b = tr[i:i + a.batch]
            _, cache = net.forward(idx[b].reshape(-1, 12), dense[b].reshape(-1, DENSE))
            z = raw(cache).reshape(-1, k) + base[b]
            w = weight[b]
            norm = max(1.0, w.sum())
            # perte log(1 + exp(z_0 − z_j)) ; s = σ(z_0 − z_j)
            s = 1 / (1 + np.exp(-(z[:, :1] - z[:, 1:])))
            dz = np.zeros_like(z)
            dz[:, 0] = (w * s).sum(1) / norm
            dz[:, 1:] = -w * s / norm
            net.backward(cache, dz.ravel(), a.lr, a.l2)
        acc = accuracy(logits(vrows), ok[vrows])
        if acc > best:
            best, best_p = acc, {key: x.copy() for key, x in net.p.items()}
        print(f"  époque {epoch + 1:>2} : validation {acc:.3f}")
    net.p = best_p
    report("Après entraînement (meilleure époque en validation)")
    if a.save:
        np.savez(a.save, **net.p)
        print(f"poids enregistrés : {a.save}")


if __name__ == "__main__":
    main()
