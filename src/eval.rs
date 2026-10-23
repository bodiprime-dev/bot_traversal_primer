//! Évaluation de Piolet, paramétrée par des poids (`Weights`).
//!
//! Point de départ : portage fidèle de `Ctx::evaluate` du SDK (niveau de compréhension 3),
//! ses constantes devenant les champs de `Weights`. Avec `Weights::default()`, le score est
//! identique au bit près à celui du SDK (voir le test `same_as_sdk`) : chaque terme ajouté
//! ensuite se mesure contre cette base.
//!
//! Termes (du point de vue de `side`) :
//!  - écart de points marqués ;
//!  - pour chaque pièce : valeur selon sa Force + avancée (tours restants pour sortir) ;
//!  - bonus de Sentinelle proche de la sortie tant que personne n'est sorti ;
//!  - course des Sentinelles (R8.2).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use traversal_bot::core::bot::{exit_costs, WIN};
use traversal_bot::core::config::{SIZE, SQUARES};
use traversal_bot::core::movegen::{neighbors_of, step_cost};
use traversal_bot::core::state::MAX_PIECES;
use traversal_bot::core::{Kind, State};

/// Poids de l'évaluation. Les valeurs par défaut sont celles du SDK (`Tuning::default()` et
/// les constantes en dur de `Ctx::evaluate`).
#[derive(Clone, Copy, Debug)]
pub struct Weights {
    /// Valeur d'un point marqué (sortie).
    pub score: i32,
    /// Valeur d'une pièce sur le plateau, par point de Force.
    pub strength: i32,
    /// Avancée : une pièce à `t` tours de la sortie vaut `(horizon - t) * (base + per_strength * F)`.
    pub progress_horizon: i32,
    pub progress_base: i32,
    pub progress_per_strength: i32,
    /// Sentinelle proche de la sortie (personne n'est sorti) : `(horizon - t)² * sentinel_sq`.
    pub sentinel_horizon: i32,
    pub sentinel_sq: i32,
    /// Course des Sentinelles gagnée / perdue, et à partir de combien de tours elle compte.
    pub sentinel_race: i32,
    pub sentinel_race_horizon: i32,
    /// Distances « avec obstacles » : traverser une alliée coûte `ally`, abattre une pièce
    /// adverse (hors Sentinelle, qui reste un mur) coûte `soft` ; le détour est plafonné à
    /// `detour` ; `blend` (en %) dose distance avec obstacles / sur le relief seul.
    pub ally: u16,
    pub soft: u16,
    pub detour: u16,
    pub blend: i32,
}

impl Default for Weights {
    fn default() -> Weights {
        Weights {
            score: 100,
            strength: 14,
            progress_horizon: 7,
            progress_base: 3,
            progress_per_strength: 2,
            sentinel_horizon: 6,
            sentinel_sq: 5,
            sentinel_race: 150,
            sentinel_race_horizon: 5,
            ally: 2,
            soft: 2,
            detour: 6,
            blend: 100,
        }
    }
}

/// Données fixes pendant un coup : le relief ne change pas, les coûts de sortie « relief
/// seul » se calculent une fois.
pub struct Eval {
    costs: [[u16; SQUARES]; 2],
    w: Weights,
}

impl Eval {
    pub fn new(heights: &[u8; SQUARES], w: Weights) -> Eval {
        Eval { costs: [exit_costs(heights, 1), exit_costs(heights, 2)], w }
    }

    #[inline]
    fn cost(&self, owner: u8, sq: u8) -> i32 {
        self.costs[owner as usize - 1][sq as usize] as i32
    }

    #[inline]
    fn turns_for(cost: i32, kind: Kind) -> i32 {
        let m = kind.movement() as i32;
        (cost + m - 1) / m
    }

    /// Coût de sortie depuis chaque case pour `owner`, en tenant compte des pièces : une
    /// Sentinelle adverse est un mur, les autres pièces adverses se franchissent en les
    /// abattant (`soft`), traverser une alliée coûte `ally`.
    fn blocked_costs(&self, state: &State, owner: u8) -> [u16; SQUARES] {
        let w = &self.w;
        let row = if owner == 1 { SIZE - 1 } else { 0 };
        let other = |sq: u8| state.at(sq).map(|i| state.piece(i)).filter(|p| p.owner != owner);
        let wall = |sq: u8| other(sq).is_some_and(|p| w.soft == 0 || p.kind == Kind::Sentinel);
        let ally = |sq: u8| state.at(sq).is_some_and(|i| state.piece(i).owner == owner);
        let mut dist = [u16::MAX; SQUARES];
        let mut heap = BinaryHeap::new();
        for x in 0..SIZE {
            let s = (row * SIZE + x) as u8;
            if !wall(s) {
                dist[s as usize] = 1;
                heap.push(Reverse((1u16, s)));
            }
        }
        while let Some(Reverse((d, cur))) = heap.pop() {
            if d > dist[cur as usize] {
                continue;
            }
            // Coût pour ENTRER sur `cur` depuis une voisine : la marche, plus la pièce à
            // écarter (alliée) ou à abattre (adverse).
            let enter = if ally(cur) {
                w.ally
            } else if other(cur).is_some() {
                w.soft
            } else {
                0
            };
            for &nb in neighbors_of(cur) {
                if wall(nb) {
                    continue;
                }
                let nd = d + step_cost(&state.heights, nb, cur) as u16 + enter;
                if nd < dist[nb as usize] {
                    dist[nb as usize] = nd;
                    heap.push(Reverse((nd, nb)));
                }
            }
        }
        dist
    }

    /// Tours (au mieux) pour sortir, pour chaque pièce, obstacles compris.
    fn piece_turns(&self, state: &State) -> [i32; MAX_PIECES] {
        let w = &self.w;
        let mut out = [0; MAX_PIECES];
        let blocked = [self.blocked_costs(state, 1), self.blocked_costs(state, 2)];
        for (i, p) in state.pieces().iter().enumerate() {
            let free = self.cost(p.owner, p.sq);
            let with = (blocked[p.owner as usize - 1][p.sq as usize] as i32).min(free + w.detour as i32);
            let cost = (free * (100 - w.blend) + with * w.blend + 50) / 100;
            out[i] = Eval::turns_for(cost, p.kind);
        }
        out
    }

    /// Évaluation du point de vue de `side`.
    pub fn evaluate(&self, state: &State, side: u8) -> i32 {
        if state.over {
            return match state.winner {
                None => 0,
                Some(w) if w == side => WIN - state.ply as i32,
                Some(_) => -WIN + state.ply as i32,
            };
        }
        let w = &self.w;
        let nobody_out = state.finished == 0;
        let other = if side == 1 { 2 } else { 1 };
        let mut total = w.score * (state.scores[side as usize - 1] as i32 - state.scores[other as usize - 1] as i32);
        let turns = self.piece_turns(state);
        let mut first = [i32::MAX; 2]; // pièce la plus rapide de chaque camp
        let mut sentinel = [i32::MAX; 2]; // Sentinelle la plus rapide de chaque camp
        for (i, p) in state.pieces().iter().enumerate() {
            let strength = p.kind.strength() as i32;
            let t = turns[i];
            let mut v = w.strength * strength
                + (w.progress_horizon - t).max(0) * (w.progress_base + w.progress_per_strength * strength);
            if p.kind == Kind::Sentinel && nobody_out {
                v += (w.sentinel_horizon - t).max(0).pow(2) * w.sentinel_sq;
            }
            total += if p.owner == side { v } else { -v };
            let o = p.owner as usize - 1;
            first[o] = first[o].min(t);
            if p.kind == Kind::Sentinel {
                sentinel[o] = sentinel[o].min(t);
            }
        }
        if nobody_out {
            total += self.sentinel_race(state, side, &first, &sentinel);
        }
        total
    }

    /// Course des Sentinelles (R8.2). Personne n'est sorti : une Sentinelle gagne si elle
    /// sort AVANT la Sentinelle adverse ET avant toute pièce adverse (une sortie adverse,
    /// même d'un Explorateur, annule la victoire immédiate). En demi-coups, le camp au trait
    /// joue le premier.
    fn sentinel_race(&self, state: &State, side: u8, first: &[i32; 2], sentinel: &[i32; 2]) -> i32 {
        let plies = |owner: u8, turns: i32| match turns {
            i32::MAX => i32::MAX,
            t if state.to_move == owner => 2 * t - 1,
            t => 2 * t,
        };
        let wins = |me: u8| {
            let them = if me == 1 { 2 } else { 1 };
            let mine = plies(me, sentinel[me as usize - 1]);
            mine < plies(them, first[them as usize - 1]) && sentinel[me as usize - 1] <= self.w.sentinel_race_horizon
        };
        let other = if side == 1 { 2 } else { 1 };
        match (wins(side), wins(other)) {
            (true, false) => self.w.sentinel_race,
            (false, true) => -self.w.sentinel_race,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use traversal_bot::core::{apply_move, legal_moves, new_game, Ctx, Tuning};

    /// Les 200 cartes officielles (`data/boards.json`, copie de celui du SDK).
    pub(crate) fn boards() -> Vec<[u8; SQUARES]> {
        include_str!("../data/boards.json")
            .split("],[")
            .map(|b| {
                let v: Vec<u8> = b
                    .trim_matches(|c| c == '[' || c == ']' || c == '\n')
                    .split(',')
                    .map(|x| x.trim().parse().expect("hauteur"))
                    .collect();
                v.try_into().expect("100 cases")
            })
            .collect()
    }

    /// Hasard reproductible (splitmix64).
    fn next(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Avec les poids par défaut, même score que le SDK sur des positions de parties
    /// aléatoires (toutes les cartes, les deux points de vue, sorties comprises).
    #[test]
    fn same_as_sdk() {
        let mut seed = 7u64;
        let (mut checked, mut after_exit) = (0, 0);
        for heights in boards() {
            let sdk = Ctx::with_insight(&heights, 3, Tuning::default());
            let ours = Eval::new(&heights, Weights::default());
            let mut s = new_game(heights);
            while !s.over {
                for side in [1, 2] {
                    assert_eq!(ours.evaluate(&s, side), sdk.evaluate(&s, side), "ply {} côté {side}", s.ply);
                    checked += 1;
                    after_exit += (s.finished > 0) as usize;
                }
                let moves = legal_moves(&s);
                s = apply_move(&s, &moves[(next(&mut seed) % moves.len() as u64) as usize]);
            }
            assert_eq!(ours.evaluate(&s, 1), sdk.evaluate(&s, 1));
        }
        assert!(checked > 10_000, "{checked} positions seulement");
        assert!(after_exit > 100, "{after_exit} positions après une sortie seulement");
        eprintln!("{checked} positions comparées, dont {after_exit} après une sortie");
    }
}
