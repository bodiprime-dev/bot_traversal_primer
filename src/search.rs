//! Moteur de recherche de Piolet : alpha-beta négamax avec
//!  - approfondissement itératif (la dernière itération complète fait foi),
//!  - PVS (fenêtre nulle sur les coups non principaux, re-recherche si nécessaire),
//!  - table de transposition (Zobrist) conservée d'un coup à l'autre pendant la partie,
//!  - recherche de calme (captures et sorties) à l'horizon,
//!  - arrêt piloté par l'appelant (`stop`), car il n'y a pas d'horloge en WebAssembly.
//!
//! L'évaluation est la nôtre (`eval.rs`, réglée par `Weights`) ; le tri des coups est encore
//! celui du SDK (`Ctx::ordered`).

use std::cmp::Reverse;

use traversal_bot::core::bot::WIN;
use traversal_bot::core::{apply_move, Ctx, Kind, Move, MoveKind, State, Tuning};

use crate::eval::{Eval, Weights};

const INF: i32 = 1_000_000_000;
/// Profondeur max de la recherche de calme (captures / sorties enchaînées).
const QPLIES: u8 = 4;
/// Taille de la table de transposition (puissance de 2).
const TT_BITS: usize = 17;

/// Réglages de la recherche.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Profondeur maximale (demi-coups) de l'approfondissement itératif.
    pub max_depth: u8,
    /// Coups « calmes » explorés par nœud, en plus des captures et sorties (toujours toutes
    /// explorées). `usize::MAX` : pas d'élagage.
    pub beam: usize,
    /// Poids de l'évaluation.
    pub weights: Weights,
    /// Coups de parade (vers une case d'un plus court chemin de la Sentinelle adverse, voir
    /// `Eval::guard_squares`) examinés en plus du faisceau : à la racine, et à chaque nœud de
    /// profondeur ≥ 2 (un Dijkstra de plus par nœud). Le tri des coups calmes ne regarde que
    /// l'avancée : sans ça, une parade qui n'avance pas est élaguée avant d'être vue.
    pub guard_root: usize,
    pub guard_tree: usize,
    /// Coups de chasse (après lesquels une Sentinelle adverse devient capturable, voir
    /// `Eval::hunts`) examinés en plus du faisceau, à la racine et à chaque nœud de
    /// profondeur ≥ 2.
    pub hunt: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config { max_depth: 16, beam: 10, weights: Weights::default(), guard_root: 0, guard_tree: 0, hunt: 0 }
    }
}

struct Timeout;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flag {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
struct Entry {
    key: u64,
    score: i32,
    mv: Option<Move>,
    depth: i8,
    flag: Flag,
}

const EMPTY: Entry = Entry { key: 0, score: 0, mv: None, depth: -1, flag: Flag::Exact };

/// Résultat d'un choix : le coup, et de quoi alimenter un message de diagnostic.
#[derive(Clone, Copy, Debug)]
pub struct Pick {
    /// Indice dans la liste de coups fournie.
    pub index: usize,
    /// Profondeur de la dernière itération complète.
    pub depth: u8,
    pub score: i32,
    pub nodes: u64,
}

/// Le bot : porte la table de transposition d'un coup à l'autre.
pub struct Piolet {
    cfg: Config,
    tt: Vec<Entry>,
    zob: Vec<u64>,
    /// Garde-fous : on repart de zéro si la position n'est pas la suite de la précédente.
    last_ply: Option<u16>,
    last_heights: Option<[u8; 100]>,
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn kind_idx(k: Kind) -> usize {
    match k {
        Kind::Explorer => 0,
        Kind::Vanguard => 1,
        Kind::Sentinel => 2,
    }
}

impl Piolet {
    pub fn new(cfg: Config) -> Piolet {
        let mut seed = 0xC0FF_EE15_600D_F00D_u64;
        // 6 types (camp × pièce) × 100 cases, + 4 clés annexes (trait, scores, sortis).
        let zob = (0..6 * 100 + 4).map(|_| splitmix(&mut seed)).collect();
        Piolet { cfg, tt: vec![EMPTY; 1 << TT_BITS], zob, last_ply: None, last_heights: None }
    }

    fn hash(&self, s: &State) -> u64 {
        let mut h = 0u64;
        for p in s.pieces() {
            h ^= self.zob[((p.owner as usize - 1) * 3 + kind_idx(p.kind)) * 100 + p.sq as usize];
        }
        if s.to_move == 2 {
            h ^= self.zob[600];
        }
        h ^= self.zob[601].wrapping_mul(1 + s.scores[0] as u64);
        h ^= self.zob[602].wrapping_mul(1 + s.scores[1] as u64);
        h ^= self.zob[603].wrapping_mul(1 + s.finished as u64);
        h
    }

    /// Choisit un coup parmi `moves` (la liste légale fournie par l'arène).
    /// `stop` est interrogé tous les 64 nœuds ; à `true`, la recherche s'arrête et on garde
    /// la dernière itération complète.
    pub fn choose(&mut self, state: &State, moves: &[Move], stop: fn() -> bool) -> Pick {
        // Nouvelle partie (ou autre relief) : la table n'a plus de sens.
        let fresh = match (self.last_ply, self.last_heights) {
            (Some(p), Some(h)) => state.ply <= p || h != state.heights,
            _ => true,
        };
        if fresh {
            self.tt.iter_mut().for_each(|e| *e = EMPTY);
        }
        self.last_ply = Some(state.ply);
        self.last_heights = Some(state.heights);

        let none = Pick { index: 0, depth: 0, score: 0, nodes: 0 };
        if moves.len() <= 1 {
            return none;
        }
        let ctx = Ctx::with_insight(&state.heights, 3, Tuning::default());
        let eval = Eval::new(&state.heights, self.cfg.weights);
        let mut s = Search { owner: self, ctx: &ctx, eval: &eval, stop, nodes: 0 };
        s.root(state, moves)
    }
}

struct Search<'a> {
    owner: &'a mut Piolet,
    ctx: &'a Ctx,
    eval: &'a Eval,
    stop: fn() -> bool,
    nodes: u64,
}

impl Search<'_> {
    fn tick(&mut self) -> Result<(), Timeout> {
        self.nodes += 1;
        if self.nodes % 64 == 0 && (self.stop)() {
            return Err(Timeout);
        }
        Ok(())
    }

    fn same(a: &Move, b: &Move) -> bool {
        a.piece == b.piece && a.kind == b.kind && a.to == b.to
    }

    fn root(&mut self, state: &State, moves: &[Move]) -> Pick {
        // Coups racine, les plus prometteurs d'abord (tri du moteur), avec leur indice.
        let mut root: Vec<(Move, usize)> = self
            .ctx
            .ordered(state)
            .into_iter()
            .map(|m| {
                let i = moves.iter().position(|x| Self::same(x, &m)).unwrap_or(0);
                (m, i)
            })
            .collect();
        let mut best = Pick { index: root[0].1, depth: 0, score: 0, nodes: 0 };
        let mut best_mv = root[0].0;
        let beam = self.owner.cfg.beam;
        // Parades hors faisceau qui restent examinées à toutes les itérations.
        let guards: Vec<Move> = if self.owner.cfg.guard_root > 0 {
            let squares = self.eval.guard_squares(state, if state.to_move == 1 { 2 } else { 1 });
            root.iter()
                .skip(beam + 6)
                .map(|&(m, _)| m)
                .filter(|m| m.kind == MoveKind::Move && squares[m.to as usize])
                .take(self.owner.cfg.guard_root)
                .collect()
        } else {
            Vec::new()
        };
        // Coups de chasse hors faisceau, examinés eux aussi à toutes les itérations.
        let hunts: Vec<Move> = if self.owner.cfg.hunt > 0 {
            root.iter()
                .skip(beam + 6)
                .map(|&(m, _)| m)
                .filter(|m| self.eval.hunts(state, m))
                .take(self.owner.cfg.hunt)
                .collect()
        } else {
            Vec::new()
        };

        'deepen: for depth in 1..=self.owner.cfg.max_depth {
            let mut scored: Vec<(i32, (Move, usize))> = Vec::with_capacity(root.len());
            let mut alpha = -INF;
            for (n, &(m, idx)) in root.iter().enumerate() {
                // Au-delà du faisceau, un coup calme n'est essayé qu'aux premières itérations.
                if n >= beam && depth > 2 && m.kind == MoveKind::Move && n >= beam + 6 && !guards.contains(&m) && !hunts.contains(&m) {
                    scored.push((-INF, (m, idx)));
                    continue;
                }
                let child = apply_move(state, &m);
                let r = if n == 0 {
                    self.negamax(&child, depth - 1, -INF, INF)
                } else {
                    // Fenêtre nulle d'abord ; re-recherche pleine si le coup améliore alpha.
                    match self.negamax(&child, depth - 1, -alpha - 1, -alpha) {
                        Ok(v) if -v > alpha => self.negamax(&child, depth - 1, -INF, -alpha),
                        other => other,
                    }
                };
                match r {
                    Ok(v) => {
                        let score = -v;
                        scored.push((score, (m, idx)));
                        alpha = alpha.max(score);
                    }
                    Err(Timeout) => {
                        // Itération interrompue : on garde son meilleur coup seulement s'il a
                        // été entièrement évalué ET dépasse le coup principal de l'itération
                        // précédente (qui est toujours essayé en premier).
                        if let Some(&(sc, (mv, i))) = scored.iter().max_by_key(|(sc, _)| *sc) {
                            if n > 0 && mv != best_mv && sc > best.score {
                                best = Pick { index: i, depth: depth - 1, score: sc, nodes: self.nodes };
                            }
                        }
                        break 'deepen;
                    }
                }
            }
            // Itération complète : tri par score (stable : l'ordre précédent départage).
            scored.sort_by_key(|(sc, _)| Reverse(*sc));
            let (score, (mv, idx)) = scored[0];
            best = Pick { index: idx, depth, score, nodes: self.nodes };
            best_mv = mv;
            root = scored.into_iter().map(|(_, x)| x).collect();
            // Victoire forcée trouvée : inutile d'aller plus loin.
            if score >= WIN - 1000 {
                break;
            }
        }
        best.nodes = self.nodes;
        best
    }

    fn probe(&self, key: u64) -> Option<Entry> {
        let e = self.owner.tt[(key as usize) & ((1 << TT_BITS) - 1)];
        (e.key == key && e.depth >= 0).then_some(e)
    }

    fn store(&mut self, key: u64, depth: u8, score: i32, flag: Flag, mv: Option<Move>) {
        let slot = &mut self.owner.tt[(key as usize) & ((1 << TT_BITS) - 1)];
        // Remplacement : on garde l'entrée plus profonde de la même position.
        if slot.key == key && slot.depth > depth as i8 && flag != Flag::Exact {
            return;
        }
        *slot = Entry { key, score, mv, depth: depth as i8, flag };
    }

    fn negamax(&mut self, state: &State, depth: u8, mut alpha: i32, mut beta: i32) -> Result<i32, Timeout> {
        if state.over {
            return Ok(self.eval.evaluate(state, state.to_move));
        }
        if depth == 0 {
            return self.quiesce(state, alpha, beta, QPLIES);
        }
        self.tick()?;

        // Près de la limite de coups, le score dépend du nombre de demi-coups restants :
        // la table (indexée sans lui) n'est pas fiable.
        let use_tt = state.ply as u32 + depth as u32 + QPLIES as u32 + 2 < state.move_limit as u32;
        let key = self.owner.hash(state);
        let (alpha0, mut tt_move) = (alpha, None);
        if use_tt {
            if let Some(e) = self.probe(key) {
                tt_move = e.mv;
                if e.depth >= depth as i8 {
                    match e.flag {
                        Flag::Exact => return Ok(e.score),
                        Flag::Lower => alpha = alpha.max(e.score),
                        Flag::Upper => beta = beta.min(e.score),
                    }
                    if alpha >= beta {
                        return Ok(e.score);
                    }
                }
            }
        }

        let mut moves = self.ctx.ordered(state);
        if let Some(t) = tt_move {
            if let Some(i) = moves.iter().position(|m| *m == t) {
                moves[..=i].rotate_right(1);
            }
        }

        let guard_tree = self.owner.cfg.guard_tree;
        let squares = (guard_tree > 0 && depth >= 2)
            .then(|| self.eval.guard_squares(state, if state.to_move == 1 { 2 } else { 1 }));
        let (mut best, mut best_move) = (-INF, None);
        let (mut quiet, mut guarded, mut hunted) = (0usize, 0usize, 0usize);
        let hunt = if depth >= 2 { self.owner.cfg.hunt } else { 0 };
        for (n, m) in moves.iter().enumerate() {
            if m.kind == MoveKind::Move {
                // Coups calmes : seuls les `beam` premiers (hors coup de la table) sont explorés,
                // plus `guard_tree` parades et `hunt` coups de chasse.
                if n > 0 && quiet >= self.owner.cfg.beam {
                    if guarded < guard_tree && squares.is_some_and(|s| s[m.to as usize]) {
                        guarded += 1;
                    } else if hunted < hunt && self.eval.hunts(state, m) {
                        hunted += 1;
                    } else {
                        continue;
                    }
                }
                quiet += 1;
            }
            let child = apply_move(state, m);
            let score = if n == 0 {
                -self.negamax(&child, depth - 1, -beta, -alpha)?
            } else {
                let v = -self.negamax(&child, depth - 1, -alpha - 1, -alpha)?;
                if v > alpha && v < beta {
                    -self.negamax(&child, depth - 1, -beta, -alpha)?
                } else {
                    v
                }
            };
            if score > best {
                best = score;
                best_move = Some(*m);
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }

        if use_tt {
            let flag = if best <= alpha0 {
                Flag::Upper
            } else if best >= beta {
                Flag::Lower
            } else {
                Flag::Exact
            };
            self.store(key, depth, best, flag, best_move);
        }
        Ok(best)
    }

    /// Au bout de l'horizon : on poursuit les captures et les sorties jusqu'au calme. Le camp
    /// au trait peut « ne rien faire » : son évaluation sur place sert de plancher.
    fn quiesce(&mut self, state: &State, mut alpha: i32, beta: i32, plies: u8) -> Result<i32, Timeout> {
        let stand = self.eval.evaluate(state, state.to_move);
        if state.over || plies == 0 || stand >= beta {
            return Ok(stand);
        }
        self.tick()?;
        alpha = alpha.max(stand);
        let mut best = stand;
        for m in self.ctx.ordered(state) {
            if !matches!(m.kind, MoveKind::Exit | MoveKind::Attack) {
                break; // triés : sorties et captures d'abord
            }
            let v = -self.quiesce(&apply_move(state, &m), -beta, -alpha, plies - 1)?;
            best = best.max(v);
            alpha = alpha.max(v);
            if alpha >= beta {
                break;
            }
        }
        Ok(best)
    }
}
