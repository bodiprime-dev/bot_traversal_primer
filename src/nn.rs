//! Piolet-NN : entrées du réseau de neurones.
//!
//! La même fonction (`features`) sert à générer les données d'entraînement et à jouer : le
//! réseau voit en partie exactement ce qu'il a vu à l'entraînement.
//!
//! Point de vue du camp au trait : les 200 cartes et la mise en place sont invariantes par
//! rotation de 180° (la case `s` de Rouge est la case `99 − s` de Bleu). Bleu voit donc la
//! position tournée : pour le réseau, « mes » pièces montent toujours vers la rangée 9.
//!
//! - `SPARSE` entrées binaires : (camp relatif, type, case tournée) — au plus 12 actives.
//! - `DENSE` entrées numériques, normalisées autour de [0, 1] : par camp et par type, nombre de
//!   pièces, tours restants de la plus proche de la sortie (blocages compris), avance cumulée ;
//!   puis marge de la course en temps, scores, sorties, demi-coup, Sentinelles capturables.

use traversal_bot::core::config::SQUARES;
use traversal_bot::core::{Kind, State};

use crate::eval::Eval;

/// (moi / adversaire) × 3 types × 100 cases.
pub const SPARSE: usize = 2 * 3 * SQUARES;
pub const DENSE: usize = 25;

fn kind_idx(k: Kind) -> usize {
    k.strength() as usize - 1 // Explorateur 0, Avant-garde 1, Sentinelle 2
}

/// Entrées du réseau pour la position, du point de vue du camp au trait : indices des entrées
/// binaires actives, et entrées numériques.
pub fn features(eval: &Eval, state: &State) -> (Vec<u16>, [f32; DENSE]) {
    let me = state.to_move;
    let them = if me == 1 { 2 } else { 1 };
    let rot = |sq: u8| if me == 1 { sq as usize } else { SQUARES - 1 - sq as usize };
    let rel = |owner: u8| if owner == me { 0 } else { 1 };

    let mut sparse = Vec::with_capacity(12);
    for p in state.pieces() {
        sparse.push(((rel(p.owner) * 3 + kind_idx(p.kind)) * SQUARES + rot(p.sq)) as u16);
    }

    let turns = eval.piece_turns(state);
    // [camp relatif][type] : nombre, tours de la plus proche (plafonnés à 15), avance cumulée.
    let mut count = [[0f32; 3]; 2];
    let mut best = [[15i32; 3]; 2];
    let mut ahead = [[0f32; 3]; 2];
    for (i, p) in state.pieces().iter().enumerate() {
        let (r, k) = (rel(p.owner), kind_idx(p.kind));
        count[r][k] += 1.0;
        best[r][k] = best[r][k].min(turns[i].min(15));
        ahead[r][k] += (7 - turns[i]).max(0) as f32;
    }
    let mut dense = [0f32; DENSE];
    let mut n = 0;
    for r in 0..2 {
        for k in 0..3 {
            dense[n] = count[r][k] / 2.0;
            dense[n + 1] = best[r][k] as f32 / 15.0;
            dense[n + 2] = ahead[r][k] / 14.0;
            n += 3;
        }
    }
    // Course en temps : marge en demi-coups (trait inclus) entre les meilleures Sentinelles.
    let sentinel = |o: u8| {
        state.pieces().iter().enumerate().filter(|(_, p)| p.owner == o && p.kind == Kind::Sentinel).map(|(i, _)| turns[i]).min()
    };
    let margin = match (sentinel(me), sentinel(them)) {
        (Some(a), Some(b)) => (Eval::plies(state, them, b) - Eval::plies(state, me, a)).clamp(-10, 10),
        (Some(_), None) => 10,
        (None, Some(_)) => -10,
        (None, None) => 0,
    };
    dense[n] = margin as f32 / 10.0;
    dense[n + 1] = state.scores[me as usize - 1] as f32 / 3.0;
    dense[n + 2] = state.scores[them as usize - 1] as f32 / 3.0;
    dense[n + 3] = state.finished as f32 / 2.0;
    dense[n + 4] = state.ply as f32 / state.move_limit.max(1) as f32;
    let threatened = |o: u8| {
        state.pieces().iter().filter(|p| p.owner == o && p.kind == Kind::Sentinel && eval.capturable(state, **p)).count() as f32 / 2.0
    };
    dense[n + 5] = threatened(me);
    dense[n + 6] = threatened(them);
    debug_assert_eq!(n + 7, DENSE);
    (sparse, dense)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards;
    use crate::eval::Weights;
    use traversal_bot::core::{apply_move, legal_moves, new_game};

    /// Rouge au trait au départ et Bleu au trait après un coup miroir voient la même chose :
    /// la rotation de 180° rend les deux points de vue identiques.
    #[test]
    fn both_sides_see_the_same_start() {
        for heights in boards().into_iter().take(20) {
            let e = Eval::new(&heights, Weights::default());
            let red = new_game(heights);
            let mut blue = red.clone();
            blue.to_move = 2;
            let (mut a, da) = features(&e, &red);
            let (mut b, db) = features(&e, &blue);
            a.sort();
            b.sort();
            assert_eq!(a, b);
            // Seule différence : la course en temps (le trait change de camp).
            assert_eq!(da[..18], db[..18]);
        }
    }

    #[test]
    fn sparse_indices_are_in_range() {
        let heights = boards()[3];
        let e = Eval::new(&heights, Weights::default());
        let mut s = new_game(heights);
        for _ in 0..30 {
            let (sparse, dense) = features(&e, &s);
            assert!(sparse.iter().all(|&i| (i as usize) < SPARSE));
            assert!(dense.iter().all(|v| v.is_finite() && (-1.0..=1.5).contains(v)), "{dense:?}");
            if s.over {
                break;
            }
            s = apply_move(&s, &legal_moves(&s)[0]);
        }
    }
}

/// Réseau entraîné par `nn/train.py`, exporté par `nn/export.py` (format `PNN1`).
pub struct Net {
    h1: usize,
    h2: usize,
    w1: Vec<f32>,
    wd: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: Vec<f32>,
    w3: Vec<f32>,
    b3: f32,
}

/// Tailles maximales des couches cachées (activations sur la pile, sans allocation).
const MAX_H1: usize = 256;
const MAX_H2: usize = 64;

impl Net {
    pub fn from_bytes(b: &[u8]) -> Result<Net, String> {
        if b.len() < 20 || &b[..4] != b"PNN1" {
            return Err("format inconnu (PNN1 attendu)".into());
        }
        let u = |i: usize| u32::from_le_bytes(b[4 + 4 * i..8 + 4 * i].try_into().unwrap()) as usize;
        let (sparse, dense, h1, h2) = (u(0), u(1), u(2), u(3));
        if sparse != SPARSE || dense != DENSE || h1 > MAX_H1 || h2 > MAX_H2 {
            return Err(format!("tailles {sparse}+{dense} -> {h1} -> {h2} incompatibles"));
        }
        let mut floats = b[20..].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap()));
        let mut take = |n: usize| -> Result<Vec<f32>, String> {
            let v: Vec<f32> = floats.by_ref().take(n).collect();
            if v.len() == n {
                Ok(v)
            } else {
                Err("fichier tronqué".into())
            }
        };
        Ok(Net {
            h1,
            h2,
            w1: take((SPARSE + 1) * h1)?,
            wd: take(DENSE * h1)?,
            b1: take(h1)?,
            w2: take(h1 * h2)?,
            b2: take(h2)?,
            w3: take(h2)?,
            b3: take(1)?[0],
        })
    }

    /// Logit de la probabilité que le camp au trait gagne.
    pub fn logit(&self, sparse: &[u16], dense: &[f32; DENSE]) -> f32 {
        let (h1, h2) = (self.h1, self.h2);
        let mut a1 = [0f32; MAX_H1];
        a1[..h1].copy_from_slice(&self.b1);
        for &i in sparse {
            let row = &self.w1[i as usize * h1..(i as usize + 1) * h1];
            for (a, w) in a1[..h1].iter_mut().zip(row) {
                *a += w;
            }
        }
        for (d, &x) in dense.iter().enumerate() {
            if x != 0.0 {
                let row = &self.wd[d * h1..(d + 1) * h1];
                for (a, w) in a1[..h1].iter_mut().zip(row) {
                    *a += x * w;
                }
            }
        }
        let mut a2 = [0f32; MAX_H2];
        a2[..h2].copy_from_slice(&self.b2);
        for (h, &a) in a1[..h1].iter().enumerate() {
            if a > 0.0 {
                let row = &self.w2[h * h2..(h + 1) * h2];
                for (b, w) in a2[..h2].iter_mut().zip(row) {
                    *b += a * w;
                }
            }
        }
        self.b3 + a2[..h2].iter().zip(&self.w3).map(|(&a, w)| a.max(0.0) * w).sum::<f32>()
    }
}

/// Le réseau du bot : `nn/weights.bin`, intégré à la compilation. En natif, la variable
/// d'environnement `PIOLET_NN` (chemin d'un autre fichier) le remplace, pour comparer des
/// réseaux sans recompiler.
pub fn net() -> &'static Net {
    static NET: std::sync::OnceLock<Net> = std::sync::OnceLock::new();
    NET.get_or_init(|| {
        let bytes = match std::env::var("PIOLET_NN") {
            Ok(path) => std::fs::read(&path).unwrap_or_else(|e| panic!("PIOLET_NN={path} : {e}")),
            Err(_) => include_bytes!("../nn/weights.bin").to_vec(),
        };
        Net::from_bytes(&bytes).expect("réseau")
    })
}

#[cfg(test)]
mod net_tests {
    use super::*;

    /// L'inférence du bot redonne les prédictions de numpy (nn/check.csv, écrit par export.py).
    #[test]
    fn matches_numpy() {
        let net = Net::from_bytes(include_bytes!("../nn/weights.bin")).unwrap();
        for line in include_str!("../nn/check.csv").lines() {
            let f: Vec<&str> = line.split(';').collect();
            let dense: Vec<f32> = f[0].split(',').map(|v| v.parse().unwrap()).collect();
            let sparse: Vec<u16> = f[1].split(',').map(|v| v.parse().unwrap()).collect();
            let expected: f32 = f[2].parse().unwrap();
            let got = net.logit(&sparse, &dense.try_into().unwrap());
            assert!((got - expected).abs() < 1e-3, "{got} au lieu de {expected}");
        }
    }

    #[test]
    fn rejects_bad_files() {
        assert!(Net::from_bytes(b"XXXX").is_err());
        let b = include_bytes!("../nn/weights.bin");
        assert!(Net::from_bytes(&b[..b.len() - 4]).is_err());
    }
}
