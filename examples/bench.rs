//! Coût des briques appelées à chaque nœud de la recherche (natif), pour savoir quoi accélérer.
//!
//! ```text
//! cargo run --release --example bench
//! ```

use std::hint::black_box;
use std::time::Instant;

use piolet::boards;
use piolet::eval::{Eval, Weights};
use traversal_bot::core::{apply_move, legal_moves, new_game, Ctx, State, Tuning};

fn main() {
    // Positions de milieu de partie : 20 coups légaux au hasard depuis le départ, sur 50 cartes.
    let mut states: Vec<State> = Vec::new();
    for (b, heights) in boards().into_iter().take(50).enumerate() {
        let mut s = new_game(heights);
        let mut seed = b as u64 * 7919 + 1;
        for _ in 0..20 {
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
    let reps = 400;
    let time = |name: &str, f: &dyn Fn(&State) -> u64| {
        let t = Instant::now();
        let mut acc = 0u64;
        for _ in 0..reps {
            for s in &states {
                acc = acc.wrapping_add(f(s));
            }
        }
        black_box(acc);
        let ns = t.elapsed().as_nanos() as f64 / (reps * states.len()) as f64;
        println!("  {name:<38} {ns:>8.0} ns");
    };
    println!("{} positions × {reps}", states.len());
    let evals: Vec<(Eval, Ctx)> = states
        .iter()
        .map(|s| {
            (
                Eval::new(&s.heights, Weights::default()),
                Ctx::with_insight(&s.heights, 3, Tuning::default()),
            )
        })
        .collect();
    let idx = |s: &State| states.iter().position(|x| std::ptr::eq(x, s)).unwrap();
    time("évaluation (evaluate)", &|s| {
        evals[idx(s)].0.evaluate(s, s.to_move) as u64
    });
    time("coups légaux (legal_moves)", &|s| {
        legal_moves(s).len() as u64
    });
    time("coups triés (Ctx::ordered)", &|s| {
        evals[idx(s)].1.ordered(s).len() as u64
    });
    time("jouer un coup (apply_move)", &|s| {
        apply_move(s, &legal_moves(s)[0]).ply as u64
    });
    time("référence : retrouver l'indice", &|s| idx(s) as u64);
}
