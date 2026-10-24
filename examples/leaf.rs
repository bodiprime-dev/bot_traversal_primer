//! Piolet-NN : données prises sur les FEUILLES de la recherche (TDLeaf).
//!
//! ```text
//! cargo run --release --example leaf -- --first 0 --games 50 > lot.csv
//! ```
//!
//! Le réseau est appelé par la recherche sur ses feuilles, pas sur les positions des parties. À
//! chaque position d'une partie d'auto-jeu, on lance une recherche courte (`--shallow`, 3 000 nœuds)
//! et une recherche profonde (`--nodes`, 20 000). On prend la FEUILLE DE LA VARIANTE PRINCIPALE de la
//! recherche courte (suite des meilleurs coups de sa table de transposition) : c'est la position dont
//! l'évaluation fait le score de la recherche courte. L'étiquette est le score de la recherche
//! profonde, ramené au point de vue du camp qui joue dans cette feuille : ce que l'évaluation
//! de la feuille devrait valoir pour que la recherche courte voie aussi loin que la profonde.
//!
//! Format identique à `examples/dataset.rs` (pour `nn/distill.py`) :
//! `carte;0;étiquette;statique;dense…;actives…`

use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use piolet::boards;
use piolet::eval::{Eval, Weights};
use piolet::nn::features;
use piolet::search::{Config, Piolet};
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
}

fn play(board: usize, heights: [u8; 100], o: &Opts, mut seed: u64) -> Vec<String> {
    let eval = Eval::new(&heights, Weights::default());
    let mut deep = [
        Piolet::new(Config::default()),
        Piolet::new(Config::default()),
    ];
    let mut shallow = [
        Piolet::new(Config::default()),
        Piolet::new(Config::default()),
    ];
    let mut s = new_game(heights);
    let mut rows = Vec::new();
    while !s.over {
        let moves = legal_moves(&s);
        if s.ply < o.random || moves.len() <= 1 {
            let quiet: Vec<_> = moves.iter().filter(|m| m.kind != MoveKind::Pass).collect();
            let mv = if quiet.is_empty() {
                moves[0]
            } else {
                *quiet[(next(&mut seed) % quiet.len() as u64) as usize]
            };
            s = apply_move(&s, &mv);
            continue;
        }
        let side = s.to_move as usize - 1;
        CALLS_LEFT.with(|c| c.set(o.deep.div_ceil(64)));
        let pick = deep[side].choose(&s, &moves, out_of_nodes);
        CALLS_LEFT.with(|c| c.set(o.shallow.div_ceil(64)));
        let witness = shallow[side].choose(&s, &moves, out_of_nodes);

        if pick.score.abs() < WIN - 1000 && witness.depth >= 2 {
            let (leaf, n) =
                shallow[side].pv_leaf(&s, &moves[witness.index], witness.depth as usize + 2);
            if !leaf.over {
                // Score profond du camp qui joue à la racine -> camp qui joue dans la feuille.
                let label = if n % 2 == 0 { pick.score } else { -pick.score };
                let (sparse, dense) = features(&eval, &leaf);
                let dense: Vec<String> = dense.iter().map(|v| format!("{v:.4}")).collect();
                let sparse: Vec<String> = sparse.iter().map(|v| v.to_string()).collect();
                rows.push(format!(
                    "{board};0;{label};{};{};{}",
                    eval.evaluate(&leaf, leaf.to_move),
                    dense.join(","),
                    sparse.join(",")
                ));
            }
        }
        s = apply_move(&s, &moves[pick.index]);
    }
    rows
}

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map_or(default, |v| v.parse().expect("nombre"))
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
                let rows = play(
                    g % boards.len(),
                    boards[g % boards.len()],
                    &o,
                    seed0.wrapping_mul(1_000_003) + g as u64,
                );
                let mut out = out.lock().unwrap();
                for r in rows {
                    writeln!(out, "{r}").unwrap();
                }
                out.flush().unwrap();
            });
        }
    });
}
