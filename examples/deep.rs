//! Piolet-NN : positions « profondes et intéressantes » pour apprendre à choisir le coup.
//!
//! ```text
//! cargo run --release --example deep -- --first 0 --games 50 --nodes 20000 > lot.csv
//! ```
//!
//! Auto-jeu de Piolet avec une recherche profonde (`--nodes`, 20 000 par défaut) ; les
//! `--random` premiers demi-coups sont tirés au hasard. À chaque position, une recherche courte
//! (`--shallow`, 3 000 nœuds) sert de témoin. On garde la position si elle est
//! - critique : le meilleur coup devance le deuxième d'au moins `--gap` points, ou
//! - difficile : le coup de la recherche courte est au moins `--gap` points sous le meilleur ;
//! et si elle n'est pas déjà résolue (victoire forcée).
//!
//! Une ligne par position : `carte;demi-coup;difficile;écart;c1 c2 … c8` où chaque candidat est
//! `score|statique|dense|actives` : la position APRÈS le coup, vue du camp qui va jouer
//! (l'adversaire), son score profond vu de ce camp-là (`UNSEARCHED` = élagué), et la valeur de
//! notre évaluation actuelle. Le premier candidat est le meilleur coup.

use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use piolet::boards;
use piolet::eval::{Eval, Weights};
use piolet::nn::features;
use piolet::search::{Config, Piolet, Pick};
use traversal_bot::core::bot::WIN;
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

struct Opts {
    deep: u64,
    shallow: u64,
    random: u16,
    gap: i32,
    k: usize,
}

fn play(board: usize, heights: [u8; 100], o: &Opts, mut seed: u64) -> Vec<String> {
    let eval = Eval::new(&heights, Weights::default());
    let mut deep = [Piolet::new(Config::default()), Piolet::new(Config::default())];
    let mut shallow = [Piolet::new(Config::default()), Piolet::new(Config::default())];
    let mut s = new_game(heights);
    let mut rows = Vec::new();
    while !s.over {
        let moves = legal_moves(&s);
        if s.ply < o.random || moves.len() <= 1 {
            let quiet: Vec<_> = moves.iter().filter(|m| m.kind != MoveKind::Pass).collect();
            let mv = if quiet.is_empty() { moves[0] } else { *quiet[(next(&mut seed) % quiet.len() as u64) as usize] };
            s = apply_move(&s, &mv);
            continue;
        }
        let side = s.to_move as usize - 1;
        CALLS_LEFT.with(|c| c.set(o.deep.div_ceil(64)));
        let pick = deep[side].choose(&s, &moves, out_of_nodes);
        let scores: Vec<(usize, i32)> = deep[side].root_scores().to_vec();
        CALLS_LEFT.with(|c| c.set(o.shallow.div_ceil(64)));
        let witness = shallow[side].choose(&s, &moves, out_of_nodes);

        if scores.len() >= 2 && scores[0].1.abs() < WIN - 1000 {
            // Difficile : le coup de la recherche courte est VRAIMENT moins bon (au moins `gap`
            // points sous le meilleur, ou élagué), pas seulement un coup équivalent.
            let witness_score = scores.iter().find(|&&(i, _)| i == witness.index).map_or(Pick::UNSEARCHED, |&(_, sc)| sc);
            let hard = witness.index != scores[0].0 && (witness_score == Pick::UNSEARCHED || scores[0].1 - witness_score >= o.gap);
            let gap = if scores[1].1 == Pick::UNSEARCHED { 1000 } else { scores[0].1 - scores[1].1 };
            if hard || gap >= o.gap {
                // Candidats : le meilleur, les suivants, puis quelques-uns au hasard parmi le reste.
                let top = (o.k / 2 + 1).min(scores.len());
                let mut picked: Vec<(usize, i32)> = scores[..top].to_vec();
                let mut rest: Vec<(usize, i32)> = scores[top..].to_vec();
                while picked.len() < o.k && !rest.is_empty() {
                    let j = (next(&mut seed) % rest.len() as u64) as usize;
                    picked.push(rest.swap_remove(j));
                }
                let cands: Vec<String> = picked
                    .iter()
                    .map(|&(i, sc)| {
                        let child = apply_move(&s, &moves[i]);
                        let (sparse, dense) = features(&eval, &child);
                        let stat = eval.evaluate(&child, child.to_move);
                        // Score vu du camp qui va jouer après le coup (l'adversaire).
                        let child_score = if sc == Pick::UNSEARCHED { sc } else { -sc };
                        let dense: Vec<String> = dense.iter().map(|v| format!("{v:.4}")).collect();
                        let sparse: Vec<String> = sparse.iter().map(|v| v.to_string()).collect();
                        format!("{child_score}|{stat}|{}|{}", dense.join(","), sparse.join(","))
                    })
                    .collect();
                rows.push(format!("{board};{};{};{gap};{}", s.ply, hard as u8, cands.join(" ")));
            }
        }
        s = apply_move(&s, &moves[pick.index]);
    }
    rows
}

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map_or(default, |v| v.parse().expect("nombre"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let first = arg(&args, "--first", 0) as usize;
    let games = arg(&args, "--games", 50) as usize;
    let seed0 = arg(&args, "--seed", 1);
    let o = Opts {
        deep: arg(&args, "--nodes", 20_000),
        shallow: arg(&args, "--shallow", 3_000),
        random: arg(&args, "--random", 4) as u16,
        gap: arg(&args, "--gap", 30) as i32,
        k: arg(&args, "--k", 8) as usize,
    };
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
                let rows = play(g % boards.len(), boards[g % boards.len()], &o, seed0.wrapping_mul(1_000_003) + g as u64);
                let mut out = out.lock().unwrap();
                for r in rows {
                    writeln!(out, "{r}").unwrap();
                }
                out.flush().unwrap();
            });
        }
    });
}
