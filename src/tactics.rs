//! Coups tactiques seuls (sorties et attaques), pour la quiescence.
//!
//! Même résultat que `legal_moves` filtré sur `Exit` et `Attack` (mêmes champs, même ordre :
//! test `same_as_legal_moves`), sans construire les déplacements simples, qui sont l'essentiel
//! de la liste et que la quiescence jette aussitôt.

use traversal_bot::core::config::{
    LEAP_OVER_PIECES, SIZE, SPECIAL_MOVES_CAN_ATTACK, SQUARES, VANGUARD_CHARGE_COST,
};
use traversal_bot::core::movegen::{combat, exit_row, neighbors_of, step_cost};
use traversal_bot::core::state::NONE;
use traversal_bot::core::{Kind, Move, MoveKind, Special, State};

/// Coût de marche minimal vers chaque case vide atteignable (`NONE` sinon), comme `movegen::reach`.
fn reach(state: &State, i: u8) -> [u8; SQUARES] {
    let piece = state.piece(i);
    let budget = piece.kind.movement() as usize;
    let mut dist = [NONE; SQUARES];
    dist[piece.sq as usize] = 0;
    let mut buckets = [[0u8; 32]; 8];
    let mut counts = [0usize; 8];
    buckets[0][0] = piece.sq;
    counts[0] = 1;
    for d in 0..=budget {
        let mut k = 0;
        while k < counts[d] {
            let cur = buckets[d][k];
            k += 1;
            if dist[cur as usize] as usize != d {
                continue;
            }
            for &nb in neighbors_of(cur) {
                if state.at(nb).is_some() {
                    continue;
                }
                let nd = d + step_cost(&state.heights, cur, nb) as usize;
                if nd <= budget && nd < dist[nb as usize] as usize {
                    dist[nb as usize] = nd as u8;
                    if counts[nd] < 32 {
                        buckets[nd][counts[nd]] = nb;
                        counts[nd] += 1;
                    } else {
                        return slow_reach(state, i);
                    }
                }
            }
        }
    }
    dist
}

/// Repli (file pleine, en pratique jamais) : file de 100 cases par coût.
fn slow_reach(state: &State, i: u8) -> [u8; SQUARES] {
    let piece = state.piece(i);
    let budget = piece.kind.movement() as usize;
    let mut dist = [NONE; SQUARES];
    dist[piece.sq as usize] = 0;
    let mut buckets = [[0u8; SQUARES]; 8];
    let mut counts = [0usize; 8];
    buckets[0][0] = piece.sq;
    counts[0] = 1;
    for d in 0..=budget {
        let mut k = 0;
        while k < counts[d] {
            let cur = buckets[d][k];
            k += 1;
            if dist[cur as usize] as usize != d {
                continue;
            }
            for &nb in neighbors_of(cur) {
                if state.at(nb).is_some() {
                    continue;
                }
                let nd = d + step_cost(&state.heights, cur, nb) as usize;
                if nd <= budget && nd < dist[nb as usize] as usize {
                    dist[nb as usize] = nd as u8;
                    buckets[nd][counts[nd]] = nb;
                    counts[nd] += 1;
                }
            }
        }
    }
    dist
}

/// Attaques et sortie d'une pièce, dans l'ordre de `movegen::piece_moves`.
fn piece_tactics(state: &State, i: u8, out: &mut Vec<Move>) {
    let piece = state.piece(i);
    let budget = piece.kind.movement();
    let dist = reach(state, i);
    let mut taken = [false; SQUARES];
    let mut best = [NONE; SQUARES];
    for sq in 0..SQUARES as u8 {
        let d = dist[sq as usize];
        if d == NONE {
            continue;
        }
        if sq != piece.sq {
            taken[sq as usize] = true;
        }
        for &nb in neighbors_of(sq) {
            let Some(j) = state.at(nb) else { continue };
            if state.piece(j).owner == piece.owner {
                continue;
            }
            let cost = d + step_cost(&state.heights, sq, nb);
            if cost <= budget && cost < best[nb as usize] {
                best[nb as usize] = cost;
            }
        }
    }
    for tsq in 0..SQUARES as u8 {
        let cost = best[tsq as usize];
        if cost == NONE {
            continue;
        }
        let target = state.at(tsq).expect("cible");
        let charge = piece.kind == Kind::Vanguard && cost == VANGUARD_CHARGE_COST;
        let (attack, defense) = combat(state, i, target, charge);
        if attack >= defense {
            out.push(Move {
                piece: i,
                kind: MoveKind::Attack,
                to: tsq,
                cost,
                special: Special::None,
                attack,
                defense,
            });
            taken[tsq as usize] = true;
        }
    }
    let row = exit_row(piece.owner);
    let mut exit_cost = NONE;
    for x in 0..SIZE as u8 {
        let d = dist[(row * SIZE as u8 + x) as usize];
        if d != NONE && d + 1 <= budget && d < exit_cost {
            exit_cost = d;
        }
    }
    if piece.kind == Kind::Explorer && SPECIAL_MOVES_CAN_ATTACK {
        let (x, y) = (
            (piece.sq % SIZE as u8) as i32,
            (piece.sq / SIZE as u8) as i32,
        );
        let specials: [(Special, i32, i32); 8] = [
            (Special::Diag, 1, 1),
            (Special::Diag, 1, -1),
            (Special::Diag, -1, 1),
            (Special::Diag, -1, -1),
            (Special::Leap, 0, 2),
            (Special::Leap, 2, 0),
            (Special::Leap, 0, -2),
            (Special::Leap, -2, 0),
        ];
        for (kind, dx, dy) in specials {
            let (nx, ny) = (x + dx, y + dy);
            if !(0..SIZE as i32).contains(&nx) || !(0..SIZE as i32).contains(&ny) {
                continue;
            }
            if kind == Special::Leap && !LEAP_OVER_PIECES {
                let middle = ((y + dy / 2) * SIZE as i32 + (x + dx / 2)) as u8;
                if state.at(middle).is_some() {
                    continue;
                }
            }
            let dest = (ny * SIZE as i32 + nx) as u8;
            if taken[dest as usize] {
                continue;
            }
            match state.at(dest) {
                // Un déplacement spécial vers une case vide la « prend » aussi (sans effet ici :
                // une case vide n'est jamais la cible d'une attaque spéciale suivante).
                None => taken[dest as usize] = true,
                Some(j) if state.piece(j).owner != piece.owner => {
                    let (attack, defense) = combat(state, i, j, false);
                    if attack >= defense {
                        out.push(Move {
                            piece: i,
                            kind: MoveKind::Attack,
                            to: dest,
                            cost: 0,
                            special: kind,
                            attack,
                            defense,
                        });
                        taken[dest as usize] = true;
                    }
                }
                Some(_) => {}
            }
        }
    }
    if exit_cost != NONE {
        out.push(Move {
            piece: i,
            kind: MoveKind::Exit,
            to: NONE,
            cost: exit_cost + 1,
            special: Special::None,
            attack: 0,
            defense: 0,
        });
    }
}

/// Sorties et attaques du camp au trait, dans l'ordre de `legal_moves`.
pub fn tactical_moves(state: &State) -> Vec<Move> {
    let mut out = Vec::with_capacity(16);
    if state.over {
        return out;
    }
    for (i, p) in state.pieces().iter().enumerate() {
        if p.owner == state.to_move {
            piece_tactics(state, i as u8, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use traversal_bot::core::{apply_move, legal_moves, new_game};

    #[test]
    fn same_as_legal_moves() {
        let mut checked = 0;
        for (b, heights) in crate::boards().into_iter().enumerate() {
            let mut s = new_game(heights);
            let mut seed = b as u64 * 31 + 7;
            while !s.over && s.ply < 120 {
                let all = legal_moves(&s);
                let expected: Vec<Move> = all
                    .iter()
                    .copied()
                    .filter(|m| matches!(m.kind, MoveKind::Exit | MoveKind::Attack))
                    .collect();
                assert_eq!(
                    tactical_moves(&s),
                    expected,
                    "carte {b}, demi-coup {}",
                    s.ply
                );
                checked += 1;
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                s = apply_move(&s, &all[(seed >> 33) as usize % all.len()]);
            }
        }
        assert!(checked > 10_000);
    }
}
