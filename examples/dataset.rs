//! Piolet-NN : génération des données d'entraînement par auto-jeu.
//!
//! ```text
//! cargo run --release --example dataset -- --games 3000 --nodes 3000 > data.csv
//! cargo run --release --example dataset -- --first 3000 --games 250 > lot.csv   # lot suivant
//! ```
//!
//! Piolet (réglages par défaut) contre lui-même ; les `--random` premiers demi-coups sont joués
//! au hasard pour diversifier les parties. Une ligne par position (camp au trait) :
//! `carte;résultat;recherche;statique;dense…;actives…` où le résultat (+1 / 0 / −1), le score
//! de la recherche et l'évaluation statique sont vus du camp au trait.

use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use piolet::boards;
use piolet::eval::{Eval, Weights};
use piolet::nn::features;
use piolet::search::{Config, Piolet};
use traversal_bot::core::{apply_move, legal_moves, new_game, MoveKind};

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

fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Une position : (camp au trait, ligne sans le résultat).
fn play(board: usize, heights: [u8; 100], nodes: u64, random: u16, mut seed: u64) -> Vec<String> {
    let eval = Eval::new(&heights, Weights::default());
    let mut bots = [Piolet::new(Config::default()), Piolet::new(Config::default())];
    let mut s = new_game(heights);
    let mut rows: Vec<(u8, String)> = Vec::new();
    while !s.over {
        let moves = legal_moves(&s);
        let mv = if s.ply < random {
            let quiet: Vec<_> = moves.iter().filter(|m| m.kind != MoveKind::Pass).collect();
            *quiet[(next(&mut seed) % quiet.len() as u64) as usize]
        } else {
            CALLS_LEFT.with(|c| c.set(nodes.div_ceil(64)));
            let pick = bots[s.to_move as usize - 1].choose(&s, &moves, out_of_nodes);
            if moves.len() > 1 {
                let (sparse, dense) = features(&eval, &s);
                let dense: Vec<String> = dense.iter().map(|v| format!("{v:.4}")).collect();
                let sparse: Vec<String> = sparse.iter().map(|i| i.to_string()).collect();
                let line = format!("{};{};{};{}", pick.score, eval.evaluate(&s, s.to_move), dense.join(","), sparse.join(","));
                rows.push((s.to_move, line));
            }
            moves[pick.index]
        };
        s = apply_move(&s, &mv);
    }
    rows.into_iter()
        .map(|(mover, line)| {
            let result = match s.winner {
                None => 0,
                Some(w) if w == mover => 1,
                Some(_) => -1,
            };
            format!("{board};{result};{line}")
        })
        .collect()
}

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map_or(default, |v| v.parse().expect("nombre"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let games = arg(&args, "--games", 3000) as usize;
    // Première partie : les lots successifs prolongent la même numérotation (cartes, hasard).
    let first = arg(&args, "--first", 0) as usize;
    let nodes = arg(&args, "--nodes", 3000);
    let random = arg(&args, "--random", 4) as u16;
    let seed0 = arg(&args, "--seed", 1);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let boards = boards();
    let next_game = AtomicUsize::new(0);
    let out = Mutex::new(std::io::BufWriter::new(std::io::stdout()));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let g = first + next_game.fetch_add(1, Ordering::Relaxed);
                if g >= first + games {
                    break;
                }
                let rows = play(g % boards.len(), boards[g % boards.len()], nodes, random, seed0.wrapping_mul(1_000_003) + g as u64);
                let mut out = out.lock().unwrap();
                for r in rows {
                    writeln!(out, "{r}").unwrap();
                }
                if (g + 1) % 100 == 0 {
                    eprintln!("{} parties", g + 1);
                }
            });
        }
    });
}
