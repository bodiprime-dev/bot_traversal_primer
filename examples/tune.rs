//! Réglage automatique des poids par SPSA, en auto-jeu.
//!
//! ```text
//! cargo run --release --example tune -- --iters 500 --nodes 3600 --out tune.csv
//! cargo run --release --example duel -- --w "<la ligne finale>"     # validation
//! ```
//!
//! SPSA (Spall) : à chaque itération, chaque paramètre reçoit une perturbation ±c au hasard ;
//! θ+ et θ− s'affrontent sur `--boards` cartes tirées au hasard, chacune jouée deux fois en
//! inversant les couleurs. L'écart de score r ∈ [−1, 1] estime la pente dans la direction
//! tirée, et tous les paramètres bougent ensemble : x ← x + a_k · r / (2 c_k Δ).
//! Les paramètres sont normalisés (θ = θ0 + échelle × x) : un pas de 1 en x vaut l'échelle du
//! paramètre. a_k = a / (k + 1 + A)^0,602, c_k = c / (k + 1)^0,101 (valeurs usuelles).

use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use piolet::boards;
use piolet::eval::Weights;
use piolet::search::{Config, Piolet};
use traversal_bot::core::{apply_move, legal_moves, new_game};

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

/// Paramètres réglés : (nom, échelle = perturbation initiale, minimum, maximum).
const PARAMS: [(&str, f64, f64, f64); 10] = [
    ("value_e", 3.0, 0.0, 100.0),
    ("value_v", 4.0, 0.0, 150.0),
    ("value_s", 6.0, 0.0, 200.0),
    ("progress_e", 1.5, 0.0, 30.0),
    ("progress_v", 1.5, 0.0, 30.0),
    ("progress_s", 1.5, 0.0, 30.0),
    ("tempo_race", 5.0, 0.0, 120.0),
    ("sentinel_sq", 1.5, 0.0, 30.0),
    ("start_race", 60.0, 0.0, 1000.0),
    ("cover", 2.0, -20.0, 20.0),
];

/// Hasard reproductible (splitmix64).
fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn weights(base: &Weights, theta: &[f64]) -> Weights {
    let mut w = *base;
    for (i, (name, _, lo, hi)) in PARAMS.iter().enumerate() {
        w.set(name, theta[i].clamp(*lo, *hi).round() as i32).expect("nom");
    }
    w
}

/// Résultat pour Rouge : 1 victoire, 0,5 nulle, 0 défaite.
fn play(heights: [u8; 100], red: Weights, blue: Weights, nodes: u64) -> f64 {
    let mut bots = [Piolet::new(Config { weights: red, ..Config::default() }), Piolet::new(Config { weights: blue, ..Config::default() })];
    let mut s = new_game(heights);
    while !s.over {
        let moves = legal_moves(&s);
        CALLS_LEFT.with(|c| c.set(nodes.div_ceil(64)));
        let pick = bots[s.to_move as usize - 1].choose(&s, &moves, out_of_nodes);
        s = apply_move(&s, &moves[pick.index]);
    }
    match s.winner {
        None => 0.5,
        Some(1) => 1.0,
        Some(_) => 0.0,
    }
}

fn arg(args: &[String], name: &str, default: &str) -> String {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let iters: usize = arg(&args, "--iters", "500").parse().expect("--iters");
    let nodes: u64 = arg(&args, "--nodes", "3600").parse().expect("--nodes");
    let per_iter: usize = arg(&args, "--boards", "8").parse().expect("--boards");
    let a0: f64 = arg(&args, "--a0", "1.0").parse().expect("--a0"); // pas initial (en échelles) pour r = 1
    let c: f64 = arg(&args, "--c", "1.0").parse().expect("--c");
    let out = arg(&args, "--out", "tune.csv");
    let mut seed: u64 = arg(&args, "--seed", "1").parse().expect("--seed");
    let mut base = Weights::default();
    base.apply(&arg(&args, "--w", "")).expect("--w");

    let theta0: Vec<f64> = PARAMS.iter().map(|(n, ..)| base.get(n).expect("nom") as f64).collect();
    let scale: Vec<f64> = PARAMS.iter().map(|p| p.1).collect();
    let mut x = vec![0.0f64; PARAMS.len()];
    let big_a = iters as f64 / 10.0;
    let a = a0 * (1.0 + big_a).powf(0.602);
    let boards = boards();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());

    let mut csv = std::fs::File::create(&out).expect("--out");
    writeln!(csv, "iter,r,{}", PARAMS.iter().map(|p| p.0).collect::<Vec<_>>().join(",")).unwrap();
    for k in 0..iters {
        let ck = c / ((k + 1) as f64).powf(0.101);
        let ak = a / ((k + 1) as f64 + big_a).powf(0.602);
        let delta: Vec<f64> = (0..PARAMS.len()).map(|_| if next(&mut seed) & 1 == 0 { 1.0 } else { -1.0 }).collect();
        let theta = |sign: f64| -> Vec<f64> { (0..PARAMS.len()).map(|i| theta0[i] + scale[i] * (x[i] + sign * ck * delta[i])).collect() };
        let (plus, minus) = (weights(&base, &theta(1.0)), weights(&base, &theta(-1.0)));
        let picked: Vec<usize> = (0..per_iter).map(|_| (next(&mut seed) % boards.len() as u64) as usize).collect();

        // 2 parties par carte (θ+ en Rouge puis en Bleu), en parallèle.
        let jobs: Vec<(usize, bool)> = picked.iter().flat_map(|&b| [(b, true), (b, false)]).collect();
        let next_job = AtomicUsize::new(0);
        let total = Mutex::new(0.0f64);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| loop {
                    let j = next_job.fetch_add(1, Ordering::Relaxed);
                    let Some(&(b, plus_red)) = jobs.get(j) else { break };
                    let score = if plus_red { play(boards[b], plus, minus, nodes) } else { 1.0 - play(boards[b], minus, plus, nodes) };
                    *total.lock().unwrap() += score;
                });
            }
        });
        let r = 2.0 * (total.into_inner().unwrap() / jobs.len() as f64) - 1.0; // ∈ [−1, 1]
        for i in 0..PARAMS.len() {
            x[i] += ak * r / (2.0 * ck * delta[i]);
            // Ne pas dériver hors des bornes.
            let (lo, hi) = (PARAMS[i].2, PARAMS[i].3);
            x[i] = x[i].clamp((lo - theta0[i]) / scale[i], (hi - theta0[i]) / scale[i]);
        }
        let now: Vec<f64> = (0..PARAMS.len()).map(|i| theta0[i] + scale[i] * x[i]).collect();
        writeln!(csv, "{k},{r:.3},{}", now.iter().map(|v| format!("{v:.2}")).collect::<Vec<_>>().join(",")).unwrap();
        csv.flush().unwrap();
        if (k + 1) % 25 == 0 || k + 1 == iters {
            let w = weights(&base, &now);
            let line: Vec<String> = PARAMS.iter().map(|(n, ..)| format!("{n}={}", w.get(n).unwrap())).collect();
            println!("itération {} : --w \"{}\"", k + 1, line.join(","));
        }
    }
}
