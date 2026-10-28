//! Profil de la recherche : Piolet cherche à nœuds fixes sur des positions de milieu de partie
//! (terrains réels de l'arène), pour `valgrind --tool=callgrind` ou un chronométrage.
//!
//! ```text
//! CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release --example prof
//! valgrind --tool=callgrind target/release/examples/prof 20 3000
//! ```

use std::cell::Cell;
use std::time::Instant;

use piolet::search::{Config, Piolet};
use traversal_bot::core::{apply_move, legal_moves, new_game, State};

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let positions: usize = args.get(1).map_or(20, |a| a.parse().unwrap());
    let nodes: u64 = args.get(2).map_or(3000, |a| a.parse().unwrap());
    let terrains = include_str!("../data/arena/terrains2.txt");
    let boards: Vec<[u8; 100]> = terrains
        .lines()
        .map(|l| {
            l.split(',')
                .map(|x| x.parse().unwrap())
                .collect::<Vec<u8>>()
                .try_into()
                .unwrap()
        })
        .collect();
    let mut states: Vec<State> = Vec::new();
    let mut seed = 12345u64;
    while states.len() < positions {
        let b = boards[states.len() % boards.len()];
        let mut s = new_game(b);
        for _ in 0..16 {
            let m = legal_moves(&s);
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s = apply_move(&s, &m[(seed >> 33) as usize % m.len()]);
            if s.over {
                break;
            }
        }
        if !s.over {
            states.push(s);
        }
    }
    let t = Instant::now();
    let (mut total, mut depth) = (0u64, 0u64);
    for s in &states {
        let mut bot = Piolet::new(Config::default());
        let moves = legal_moves(s);
        CALLS_LEFT.with(|c| c.set(nodes.div_ceil(64)));
        let p = bot.choose(s, &moves, out_of_nodes);
        total += p.nodes;
        depth += p.depth as u64;
    }
    let el = t.elapsed();
    println!(
        "{} positions, {} nœuds, profondeur moyenne {:.2}, {:.1} µs par nœud",
        states.len(),
        total,
        depth as f64 / states.len() as f64,
        el.as_secs_f64() * 1e6 / total as f64
    );
}
