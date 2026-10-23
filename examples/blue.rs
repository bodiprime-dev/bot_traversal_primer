//! Analyse : comment Piolet perd la course des Sentinelles en Bleu, et quels coups lui
//! auraient fait gagner du temps (« 2 temps contre 1 »).
//!
//! ```text
//! cargo run --release --example blue -- --games 200 --nodes 3600
//! ```
//!
//! Piolet (réglages par défaut) joue Bleu contre Mont Blanc sur chaque carte. À chacun de ses
//! tours, on mesure la course (tours restants de la Sentinelle rouge la plus menaçante et de
//! la nôtre, blocages compris : `tempo::sprint_turns`) et le bilan en temps de chaque coup
//! légal (`tempo::tempo_balance`) : gain net = tours perdus par eux + gagnés par nous − 1 (un
//! coup coûte un temps). Un coup « 2 contre 1 » a un gain net ≥ 1.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use piolet::boards;
use piolet::search::{Config, Piolet};
use piolet::tempo::{best_sentinel, tempo_balance, BLOCKED};
use traversal_bot::core::{apply_move, choose_move, legal_moves, new_game, Kind, Level, MoveKind, Rng};

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

/// Un tour de Bleu.
struct Turn {
    /// Tours restants (blocages compris) : Sentinelle rouge la plus menaçante, la nôtre.
    red: u8,
    blue: u8,
    /// Meilleur gain net disponible, gain net du coup joué.
    best: i32,
    played: i32,
    /// Le coup joué était-il un coup de Sentinelle ?
    sentinel_move: bool,
}

struct Game {
    board: usize,
    result: i32,
    reason: &'static str,
    turns: Vec<Turn>,
    /// Sentinelle rouge gagnante : tours au départ (blocages compris), coups rouges joués.
    red_start: u8,
    red_moves: usize,
}

fn net((theirs, ours): (i32, i32)) -> i32 {
    theirs.min(BLOCKED as i32) + ours - 1
}

fn play(board: usize, heights: [u8; 100], nodes: u64) -> Game {
    let mut bot = Piolet::new(Config::default());
    let mut s = new_game(heights);
    let red_start = best_sentinel(&s, 1).map_or(BLOCKED, |(_, t)| t);
    let (mut turns, mut red_moves) = (Vec::new(), 0);
    while !s.over {
        let moves = legal_moves(&s);
        let mv = if s.to_move == 2 {
            CALLS_LEFT.with(|c| c.set(nodes.div_ceil(64)));
            let mv = moves[bot.choose(&s, &moves, out_of_nodes).index];
            let red = best_sentinel(&s, 1).map_or(BLOCKED, |(_, t)| t);
            let blue = best_sentinel(&s, 2).map_or(BLOCKED, |(_, t)| t);
            let best = moves.iter().filter(|m| m.kind != MoveKind::Pass).map(|m| net(tempo_balance(&s, m))).max().unwrap_or(0);
            let played = if mv.kind == MoveKind::Pass { -1 } else { net(tempo_balance(&s, &mv)) };
            let sentinel_move = mv.kind != MoveKind::Pass && s.piece(mv.piece).kind == Kind::Sentinel;
            turns.push(Turn { red, blue, best, played, sentinel_move });
            mv
        } else {
            red_moves += 1;
            choose_move(&s, MONT_BLANC, &mut Rng::new(s.ply as u64)).mv
        };
        s = apply_move(&s, &mv);
    }
    let result = match s.winner {
        None => 0,
        Some(2) => 1,
        Some(_) => -1,
    };
    Game { board, result, reason: s.reason.map_or("?", |r| r.name()), turns, red_start, red_moves }
}

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map_or(default, |v| v.parse().expect("nombre"))
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
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
                let game = play(g, boards[g % boards.len()], nodes);
                done.lock().unwrap().push(game);
            });
        }
    });
    let mut all = done.into_inner().unwrap();
    all.sort_by_key(|g| g.board);

    let lost: Vec<&Game> = all.iter().filter(|g| g.result == -1 && g.reason == "sentinel").collect();
    let won: Vec<&Game> = all.iter().filter(|g| g.result == 1).collect();
    println!("Piolet en Bleu contre Mont Blanc : {} parties, {} gagnées, {} perdues sur la Sentinelle rouge", all.len(), won.len(), lost.len());

    for (name, set) in [("perdues (Sentinelle rouge)", &lost), ("gagnées", &won)] {
        println!("\n== Parties {name} ({})", set.len());
        // Rouge : tours au départ contre coups joués avant de sortir (gaspillage).
        let waste: Vec<f64> = set.iter().map(|g| g.red_moves as f64 - g.red_start as f64).collect();
        println!("  Rouge : {:.1} coups joués en plus du minimum de sa Sentinelle (moyenne)", mean(&waste));
        // Course au premier tour de Bleu, et au moment où Bleu est passé derrière pour de bon.
        let first_gap: Vec<f64> = set.iter().filter_map(|g| g.turns.first()).map(|t| t.blue as f64 - t.red as f64).collect();
        println!("  Au départ : notre Sentinelle a {:+.2} tours de plus que la rouge (moyenne)", mean(&first_gap));
        // Coups « 2 contre 1 » : disponibles, joués.
        let turns: Vec<&Turn> = set.iter().flat_map(|g| g.turns.iter()).collect();
        let behind: Vec<&&Turn> = turns.iter().filter(|t| t.blue > t.red).collect();
        let avail = |v: &[&&Turn]| v.iter().filter(|t| t.best >= 1).count();
        let taken = |v: &[&&Turn]| v.iter().filter(|t| t.best >= 1 && t.played >= 1).count();
        let all_t: Vec<&&Turn> = turns.iter().collect();
        println!(
            "  Tours de Bleu : {} ; avec un coup à gain net ≥ 1 disponible : {} ; joué : {}",
            turns.len(),
            avail(&all_t),
            taken(&all_t)
        );
        println!(
            "  Tours où Bleu est derrière dans la course : {} ; coup à gain net ≥ 1 disponible : {} ; joué : {}",
            behind.len(),
            avail(&behind),
            taken(&behind)
        );
        let mut hist: BTreeMap<(i32, i32), usize> = BTreeMap::new();
        for t in &turns {
            *hist.entry((t.best.min(3), t.played.clamp(-1, 3))).or_default() += 1;
        }
        println!("  (meilleur gain net dispo, gain net joué) -> tours : {hist:?}");
        let sentinel = turns.iter().filter(|t| t.sentinel_move).count();
        println!("  Coups de Sentinelle : {sentinel} / {}", turns.len());
    }

    // Quelques parties perdues à examiner : la course tour par tour.
    println!("\n== Exemples (perdues) : tour de Bleu -> (rouge, bleu, meilleur net, net joué)");
    for g in lost.iter().take(6) {
        let line: Vec<String> = g.turns.iter().map(|t| format!("({},{},{},{})", t.red, t.blue, t.best, t.played)).collect();
        println!("  carte {:>3} : {}", g.board, line.join(" "));
    }
}
