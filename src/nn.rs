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
