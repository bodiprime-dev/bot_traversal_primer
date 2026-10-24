"""Piolet-NN : distiller la recherche dans l'évaluation (correction absolue).

    python3 nn/distill.py data.csv [--epochs 15] [--hidden 64] [--save poids.npz]

Données : `examples/dataset.rs` (une ligne par position de partie, vue du camp au trait) :
    carte;résultat;recherche;statique;dense…;actives…
Cible : ce que la recherche (3 000 nœuds) ajoute à l'évaluation statique,
    t = clip((recherche − statique) / ÉCHELLE, ± --clip)            (en logit)
Le réseau prédit t (erreur quadratique) : la valeur est absolue, donc rien ne peut dériver (contrairement
à une perte par paires). Les positions déjà tranchées (victoire forcée, |recherche| > 2 000) sont écartées.
Test : cartes 160+. On compare l'erreur du réseau à celle de « ne rien corriger » (t = 0).

La correction est ensuite appliquée par le bot comme `Weights::nn_resid` : évaluation + ÉCHELLE × réseau.
"""

import argparse
import numpy as np

from train import Net, SPARSE, DENSE, load

SCALE = 270.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("data")
    ap.add_argument("--hidden", type=int, default=64)
    ap.add_argument("--epochs", type=int, default=15)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--l2", type=float, default=1e-3)
    ap.add_argument("--clip", type=float, default=1.0, help="borne de la cible (logit)")
    ap.add_argument("--batch", type=int, default=512)
    ap.add_argument("--test-from", type=int, default=160)
    ap.add_argument("--save")
    a = ap.parse_args()

    boards, y, search, static, dense, idx = load(a.data)
    keep = np.abs(search) < 2000
    boards, search, static, dense, idx = boards[keep], search[keep], static[keep], dense[keep], idx[keep]
    target = np.clip((search - static) / SCALE, -a.clip, a.clip).astype(np.float32)
    test = boards >= a.test_from
    val = ~test & (boards >= a.test_from - 20)
    train = ~test & ~val
    print(f"{len(target)} positions ({train.sum()} entraînement, {val.sum()} validation, {test.sum()} test)")
    print(f"cible : écart-type {target.std():.3f} logit = {target.std() * SCALE:.0f} points")

    rng = np.random.default_rng(1)
    net = Net(a.hidden, rng)
    net.p["W3"][:] = 0  # on part de « aucune correction »
    net.p["b3"][:] = 0

    def raw(rows):
        out = []
        for i in range(0, len(rows), 8192):
            b = rows[i:i + 8192]
            _, cache = net.forward(idx[b], dense[b])
            out.append((cache[5] @ net.p["W3"] + net.p["b3"])[:, 0])
        return np.concatenate(out)

    def mse(rows):
        return float(np.mean((raw(rows) - target[rows]) ** 2))

    tr, va, te = np.flatnonzero(train), np.flatnonzero(val), np.flatnonzero(test)
    base = lambda rows: float(np.mean(target[rows] ** 2))  # erreur de « ne rien corriger »
    best, best_p = 1e9, None
    for epoch in range(a.epochs):
        rng.shuffle(tr)
        for i in range(0, len(tr), a.batch):
            b = tr[i:i + a.batch]
            _, cache = net.forward(idx[b], dense[b])
            r = (cache[5] @ net.p["W3"] + net.p["b3"])[:, 0]
            net.backward(cache, 2 * (r - target[b]) / len(b), a.lr, a.l2)
        v = mse(va)
        if v < best:
            best, best_p = v, {k: x.copy() for k, x in net.p.items()}
        print(f"  époque {epoch + 1:>2} : validation {v:.4f} (aucune correction : {base(va):.4f})")
    net.p = best_p
    pred = raw(te)
    print(f"\ntest : erreur quadratique {mse(te):.4f} contre {base(te):.4f} sans correction "
          f"({100 * (1 - mse(te) / base(te)):.1f} % de la variance expliquée)")
    print(f"correction prédite : écart-type {pred.std() * SCALE:.0f} points (cible : {target[te].std() * SCALE:.0f}) ; "
          f"|p95| {np.percentile(np.abs(pred), 95) * SCALE:.0f} points")
    if a.save:
        np.savez(a.save, **net.p)
        print(f"poids enregistrés : {a.save}")


if __name__ == "__main__":
    main()
