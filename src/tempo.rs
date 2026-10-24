//! Temps (tempo) dans la course des Sentinelles.
//!
//! `sprint_turns` : tours exacts qu'il faut à une pièce pour sortir si plus rien d'autre ne
//! bouge — parcours tour par tour avec les vrais coups du moteur (`piece_moves`) : les pièces
//! bloquent, les captures permises par la règle de combat sont autorisées. Contrairement à la
//! distance « relief seul », elle voit les blocages.
//!
//! `tempo_balance` : ce qu'un coup coûte à la Sentinelle adverse la plus menaçante et rapporte
//! à la nôtre, en tours. Un coup qui fait perdre 2 tours à l'adversaire pour 1 coup joué (la
//! pièce déplacée n'est pas notre Sentinelle) gagne un temps net.

use traversal_bot::core::config::SQUARES;
use traversal_bot::core::movegen::piece_moves;
use traversal_bot::core::{apply_move, Kind, Move, MoveKind, Piece, State};

/// Au-delà, la pièce est considérée comme bloquée.
pub const BLOCKED: u8 = 20;

/// Tours pour que la pièce sur `sq` sorte, les autres pièces restant immobiles (`BLOCKED` si
/// elle n'y arrive pas en moins de `BLOCKED` tours).
pub fn sprint_turns(state: &State, sq: u8) -> u8 {
    let Some(i) = state.at(sq) else {
        return BLOCKED;
    };
    let runner = state.piece(i);
    let others: Vec<Piece> = state
        .pieces()
        .iter()
        .enumerate()
        .filter(|&(j, _)| j != i as usize)
        .map(|(_, p)| *p)
        .collect();
    let mut seen = [false; SQUARES];
    seen[sq as usize] = true;
    let mut frontier = vec![sq];
    let mut buf = Vec::with_capacity(64);
    for turn in 1..BLOCKED {
        let mut next = Vec::new();
        for &from in &frontier {
            // La pièce en `from` ; une pièce adverse déjà sur cette case a été capturée.
            let mut pieces: Vec<Piece> = others.iter().copied().filter(|p| p.sq != from).collect();
            pieces.push(Piece { sq: from, ..runner });
            let s = State::new(
                state.heights,
                &pieces,
                runner.owner,
                state.ply,
                state.scores,
                state.finished,
                state.move_limit,
            );
            buf.clear();
            piece_moves(&s, (pieces.len() - 1) as u8, &mut buf);
            for m in &buf {
                match m.kind {
                    MoveKind::Exit => return turn,
                    MoveKind::Move | MoveKind::Attack if !seen[m.to as usize] => {
                        seen[m.to as usize] = true;
                        next.push(m.to);
                    }
                    _ => {}
                }
            }
        }
        if next.is_empty() {
            return BLOCKED;
        }
        frontier = next;
    }
    BLOCKED
}

/// La Sentinelle de `owner` la plus proche de la sortie : (case, tours).
pub fn best_sentinel(state: &State, owner: u8) -> Option<(u8, u8)> {
    state
        .pieces()
        .iter()
        .filter(|p| p.owner == owner && p.kind == Kind::Sentinel)
        .map(|p| (p.sq, sprint_turns(state, p.sq)))
        .min_by_key(|&(_, t)| t)
}

/// Bilan d'un coup du camp au trait, en tours : (perdus par la Sentinelle adverse la plus
/// menaçante, gagnés par notre Sentinelle la plus avancée). Une Sentinelle adverse capturée
/// compte `BLOCKED`.
pub fn tempo_balance(state: &State, m: &Move) -> (i32, i32) {
    let me = state.to_move;
    let them = if me == 1 { 2 } else { 1 };
    let child = apply_move(state, m);
    let theirs = match best_sentinel(state, them) {
        None => 0,
        Some((sq, before)) => {
            // Le coup ne déplace pas leur Sentinelle : on la retrouve sur sa case, ou capturée.
            let after = match child.at(sq) {
                Some(j) if child.piece(j).owner == them => sprint_turns(&child, sq),
                _ => BLOCKED,
            };
            after as i32 - before as i32
        }
    };
    let ours = match (best_sentinel(state, me), best_sentinel(&child, me)) {
        (Some((_, before)), Some((_, after))) => before as i32 - after as i32,
        _ => 0,
    };
    (theirs, ours)
}

#[cfg(test)]
mod tests {
    use super::*;
    use traversal_bot::core::legal_moves;

    const FLAT: [u8; SQUARES] = [1; SQUARES];

    fn piece(owner: u8, kind: Kind, sq: u8) -> Piece {
        Piece { owner, kind, sq }
    }

    #[test]
    fn sprint_turns_sees_blocking_pieces() {
        // Sentinelle bleue en 35 : sur le plat, 4 de coût -> 2 tours.
        let alone = State::new(FLAT, &[piece(2, Kind::Sentinel, 35)], 2, 0, [0, 0], 0, 200);
        assert_eq!(sprint_turns(&alone, 35), 2);
        // Ligne rouge en rangée 2 : le centre (défense 4) est un mur, mais l'Explorateur du bout
        // (23, défense 2) se prend en chemin, en 3 de coût : toujours 2 tours.
        let pieces = [
            piece(2, Kind::Sentinel, 35),
            piece(1, Kind::Vanguard, 24),
            piece(1, Kind::Vanguard, 25),
            piece(1, Kind::Vanguard, 26),
            piece(1, Kind::Explorer, 23),
            piece(1, Kind::Explorer, 27),
        ];
        let line = State::new(FLAT, &pieces, 2, 0, [0, 0], 0, 200);
        assert_eq!(sprint_turns(&line, 35), 2);
    }

    #[test]
    fn a_blocker_costs_a_turn_even_if_captured() {
        // Bleu au trait. Sentinelle rouge en 75 : 75 -> 85 -> 95 -> sortie, 3 de coût, 1 tour.
        // Une Avant-garde bleue en 85 se fait prendre (3 contre 2), mais une capture TERMINE le
        // déplacement : la Sentinelle sort au tour suivant (le détour coûte 4, trop cher).
        // Un temps perdu pour Rouge contre un coup joué par Bleu.
        let pieces = [
            piece(1, Kind::Sentinel, 75),
            piece(2, Kind::Vanguard, 94),
            piece(2, Kind::Sentinel, 4),
        ];
        let s = State::new(FLAT, &pieces, 2, 20, [0, 0], 0, 200);
        assert_eq!(sprint_turns(&s, 75), 1);
        let block = legal_moves(&s)
            .into_iter()
            .find(|m| m.kind == MoveKind::Move && m.to == 85)
            .expect("coup");
        assert_eq!(tempo_balance(&s, &block), (1, 0));
    }
}
