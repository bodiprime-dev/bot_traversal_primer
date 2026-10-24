//! Analyse : pourquoi Piolet, en Bleu, refuse-t-il de capturer une Sentinelle rouge ?
//!
//! ```text
//! cargo run --release --example captures -- --games 200 --nodes 3600
//! ```
//!
//! Mêmes parties que `examples/blue.rs` (Piolet en Bleu, réglages par défaut, contre Mont
//! Blanc). À chaque tour où une capture de Sentinelle rouge était possible et n'a pas été
//! jouée : le coup joué à la place, la reprise possible après la capture, les valeurs que
//! Piolet donne aux deux positions (évaluation statique et recherche au même budget), et ce
//! qu'est devenue la Sentinelle épargnée.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use piolet::boards;
use piolet::eval::{Eval, Weights};
use piolet::search::{Config, Piolet};
use traversal_bot::core::{
    apply_move, choose_move, legal_moves, new_game, Kind, Level, Move, MoveKind, Rng, State,
};

thread_local! {
    static CALLS_LEFT: Cell<u64> = const { Cell::new(0) };
}

fn out_of_nodes() -> bool {
    CALLS_LEFT.with(|c| {
        let left = c.get().saturating_sub(1);
        c.set(left);
        left == 0
    })
}

fn never() -> bool {
    false
}

const MONT_BLANC: Level = Level {
    depth: 4,
    beam: 7,
    noise: 0.0,
    budget: Duration::MAX,
    insight: 3,
    tuning: traversal_bot::core::bot::Tuning {
        qplies: 4,
        ally: 2,
        soft: 2,
        detour: 6,
        blend: 100,
        guard_extra: 2,
        srace: 150,
        srace_horizon: 5,
    },
    stop: Some(never),
};

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Explorer => "Explorateur",
        Kind::Vanguard => "Avant-garde",
        Kind::Sentinel => "Sentinelle",
    }
}

/// Valeur d'une position du point de vue de Bleu par une recherche de Piolet au budget donné.
fn searched(state: &State, nodes: u64) -> i32 {
    if state.over {
        return Eval::new(&state.heights, Weights::default()).evaluate(state, 2);
    }
    let moves = legal_moves(state);
    CALLS_LEFT.with(|c| c.set(nodes.div_ceil(64)));
    let pick = Piolet::new(Config::default()).choose(state, &moves, out_of_nodes);
    // Rouge au trait : son score, retourné pour Bleu. (Un seul coup légal : pas de recherche.)
    if moves.len() <= 1 {
        -Eval::new(&state.heights, Weights::default()).evaluate(&apply_move(state, &moves[0]), 1)
    } else if state.to_move == 2 {
        pick.score
    } else {
        -pick.score
    }
}

struct Case {
    board: usize,
    ply: u16,
    attacker: Kind,
    played: String,
    /// Après la capture, Rouge peut-il reprendre la pièce qui a capturé ? notre Sentinelle ?
    recapture: bool,
    our_sentinel_hit: bool,
    /// Évaluation statique (pour Bleu) après la capture / après le coup joué.
    static_capture: i32,
    static_played: i32,
    /// Recherche au même budget depuis chaque position (pour Bleu).
    search_capture: i32,
    search_played: i32,
    /// La Sentinelle épargnée est-elle celle qui a fini par sortir (défaite de Bleu) ?
    decisive: bool,
}

fn describe(s: &State, m: &Move) -> String {
    match m.kind {
        MoveKind::Pass => "passe".into(),
        MoveKind::Exit => format!("sortie ({})", kind_name(s.piece(m.piece).kind)),
        MoveKind::Attack => {
            let t = s.piece(s.at(m.to).expect("cible"));
            format!(
                "capture d'un(e) {} par {}",
                kind_name(t.kind),
                kind_name(s.piece(m.piece).kind)
            )
        }
        MoveKind::Move => format!("déplacement ({})", kind_name(s.piece(m.piece).kind)),
    }
}

fn play(board: usize, heights: [u8; 100], nodes: u64) -> Vec<Case> {
    let mut bot = Piolet::new(Config::default());
    let eval = Eval::new(&heights, Weights::default());
    let mut s = new_game(heights);
    let mut pending: Vec<(Case, u8)> = Vec::new(); // (cas, case de la Sentinelle épargnée)
    let mut exits: Vec<(u8, u8)> = Vec::new(); // sorties rouges de Sentinelle : case de départ du coup
    while !s.over {
        let moves = legal_moves(&s);
        let mv = if s.to_move == 2 {
            CALLS_LEFT.with(|c| c.set(nodes.div_ceil(64)));
            let mv = moves[bot.choose(&s, &moves, out_of_nodes).index];
            let caps: Vec<Move> = moves
                .iter()
                .copied()
                .filter(|m| {
                    m.kind == MoveKind::Attack
                        && s.piece(s.at(m.to).expect("cible")).kind == Kind::Sentinel
                })
                .collect();
            if !caps.is_empty() && !caps.contains(&mv) {
                let cap = caps[0];
                let after = apply_move(&s, &cap);
                let attacker = after.at(cap.to).map(|j| after.piece(j));
                let recapture = attacker.is_some_and(|p| eval.capturable(&after, p));
                let our_sentinel_hit = after.pieces().iter().any(|p| {
                    p.owner == 2 && p.kind == Kind::Sentinel && eval.capturable(&after, *p)
                });
                let played_state = apply_move(&s, &mv);
                let case = Case {
                    board,
                    ply: s.ply,
                    attacker: s.piece(cap.piece).kind,
                    played: describe(&s, &mv),
                    recapture,
                    our_sentinel_hit,
                    static_capture: eval.evaluate(&after, 2),
                    static_played: eval.evaluate(&played_state, 2),
                    search_capture: searched(&after, nodes),
                    search_played: searched(&played_state, nodes),
                    decisive: false,
                };
                pending.push((case, cap.to));
            }
            mv
        } else {
            let mv = choose_move(&s, MONT_BLANC, &mut Rng::new(s.ply as u64)).mv;
            if mv.kind == MoveKind::Exit && s.piece(mv.piece).kind == Kind::Sentinel {
                exits.push((s.piece(mv.piece).sq, mv.piece));
            }
            mv
        };
        // Suivre la Sentinelle épargnée : si elle bouge, sa case suit.
        if s.to_move == 1 && mv.kind != MoveKind::Pass {
            let from = s.piece(mv.piece).sq;
            for (_, sq) in pending.iter_mut() {
                if *sq == from && mv.kind != MoveKind::Exit {
                    *sq = mv.to;
                }
            }
        }
        s = apply_move(&s, &mv);
    }
    let red_sentinel_won = s.winner == Some(1) && s.reason.is_some_and(|r| r.name() == "sentinel");
    pending
        .into_iter()
        .map(|(mut c, sq)| {
            c.decisive = red_sentinel_won && exits.iter().any(|&(from, _)| from == sq);
            c
        })
        .collect()
}

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map_or(default, |v| v.parse().expect("nombre"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let games = arg(&args, "--games", 200) as usize;
    let nodes = arg(&args, "--nodes", 3600);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let boards = boards();
    let next = AtomicUsize::new(0);
    let done = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let g = next.fetch_add(1, Ordering::Relaxed);
                if g >= games {
                    break;
                }
                let cases = play(g, boards[g % boards.len()], nodes);
                done.lock().unwrap().extend(cases);
            });
        }
    });
    let mut all = done.into_inner().unwrap();
    all.sort_by_key(|c| (c.board, c.ply));

    println!(
        "Captures de Sentinelle rouge refusées par Piolet (Bleu) : {} cas",
        all.len()
    );
    let count = |f: &dyn Fn(&Case) -> bool| all.iter().filter(|c| f(c)).count();
    println!(
        "  la Sentinelle épargnée a gagné la partie : {}",
        count(&|c| c.decisive)
    );
    println!(
        "  reprise possible de la pièce qui capture : {}",
        count(&|c| c.recapture)
    );
    println!(
        "  notre Sentinelle capturable après la capture : {}",
        count(&|c| c.our_sentinel_hit)
    );
    println!(
        "  évaluation statique : capture jugée meilleure : {}",
        count(&|c| c.static_capture > c.static_played)
    );
    println!(
        "  recherche au même budget : capture jugée meilleure : {}",
        count(&|c| c.search_capture > c.search_played)
    );
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for c in &all {
        *by.entry(c.played.clone()).or_default() += 1;
    }
    println!("  coup joué à la place : {by:?}");
    let mut att: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &all {
        *att.entry(kind_name(c.attacker)).or_default() += 1;
    }
    println!("  pièce qui aurait capturé : {att:?}");
    println!("\ncarte  demi-coup  capture par   reprise  S menacée  statique (cap / joué)  recherche (cap / joué)  décisif  joué");
    for c in &all {
        println!(
            "{:>5} {:>10}  {:<12} {:>7} {:>10} {:>10} / {:<10} {:>10} / {:<10} {:>7}  {}",
            c.board,
            c.ply,
            kind_name(c.attacker),
            c.recapture,
            c.our_sentinel_hit,
            c.static_capture,
            c.static_played,
            c.search_capture,
            c.search_played,
            c.decisive,
            c.played
        );
    }
}
