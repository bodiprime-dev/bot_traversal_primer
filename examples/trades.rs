//! Poids actuel d'un échange de Sentinelle dans l'évaluation, terme par terme.
//!
//! ```text
//! cargo run --release --example trades -- [--seeds 5] [--w "trade_adv=3"]
//! ```
//!
//! Positions de milieu de partie (coups au hasard depuis le départ, sur les 200 cartes). Dans
//! chaque position, pour chaque prise d'une Sentinelle suivie d'une reprise sur la même case (la
//! meilleure pour l'évaluation), on mesure ce que l'échange change à l'évaluation, du point de vue
//! du camp qui perd sa Sentinelle puis reprend : avant la prise -> après la reprise.
//!
//! Décomposition : contribution d'un groupe de termes = évaluation complète − évaluation avec ce
//! groupe à zéro. L'évaluation n'étant pas exactement linéaire, un reste est affiché.

use piolet::boards;
use piolet::eval::{Eval, Weights};
use traversal_bot::core::{apply_move, legal_moves, new_game, Kind, MoveKind, State};

const GROUPS: [&str; 7] = [
    "matériel",
    "avancée Explorateur/Avant-garde",
    "avancée Sentinelle",
    "bonus Sentinelle (sentinel_sq)",
    "course Sentinelle (sentinel_race)",
    "course à la sortie (exit_race)",
    "course en temps (tempo_race)",
];

fn without(group: usize, w: Weights) -> Weights {
    let mut w = w;
    match group {
        0 => w.value = [0; 3],
        1 => (w.progress[0], w.progress[1]) = (0, 0),
        2 => w.progress[2] = 0,
        3 => w.sentinel_sq = 0,
        4 => w.sentinel_race = 0,
        5 => w.exit_race = 0,
        _ => w.tempo_race = 0,
    }
    w
}

fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn name(k: Kind) -> &'static str {
    match k {
        Kind::Explorer => "E",
        Kind::Vanguard => "A",
        Kind::Sentinel => "S",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seeds: u64 = args
        .iter()
        .position(|a| a == "--seeds")
        .and_then(|i| args.get(i + 1))
        .map_or(5, |v| v.parse().expect("nombre"));
    let mut w = Weights::default();
    if let Some(o) = args.iter().position(|a| a == "--w").and_then(|i| args.get(i + 1)) {
        w.apply(o).expect("--w");
    }
    // sums[0] : total ; sums[1 + g] : groupe g.
    let mut sums = [0f64; 1 + GROUPS.len()];
    let mut by_pair: Pairs = Default::default();
    let mut n = 0u32;
    for (b, heights) in boards().into_iter().enumerate() {
        let full = Eval::new(&heights, w);
        let parts: Vec<Eval> = (0..GROUPS.len())
            .map(|g| Eval::new(&heights, without(g, w)))
            .collect();
        for seed in 0..seeds {
            let mut rng = (b as u64) * 1_000_003 + seed * 7919 + 1;
            let mut s: State = new_game(heights);
            // Chaque position d'une partie au hasard (8e demi-coup et au-delà) est examinée.
            for ply in 0..60 {
                if s.over {
                    break;
                }
                if ply >= 8 {
                    examine(&s, &full, &parts, &mut sums, &mut by_pair, &mut n);
                }
                let m = legal_moves(&s);
                s = apply_move(&s, &m[(next(&mut rng) % m.len() as u64) as usize]);
            }
        }
    }
    report(&sums, &by_pair, n);
}

type Pairs = std::collections::BTreeMap<String, (u32, f64)>;

fn examine(
    s: &State,
    full: &Eval,
    parts: &[Eval],
    sums: &mut [f64; 1 + GROUPS.len()],
    by_pair: &mut Pairs,
    n: &mut u32,
) {
    {
        {
            let victim = if s.to_move == 1 { 2 } else { 1 };
            for m in legal_moves(s) {
                if m.kind != MoveKind::Attack {
                    continue;
                }
                let Some(t) = s.at(m.to) else { continue };
                if s.piece(t).kind != Kind::Sentinel {
                    continue;
                }
                let attacker = s.piece(m.piece).kind;
                let after = apply_move(s, &m);
                if after.over {
                    continue;
                }
                // Meilleure reprise sur la case de la prise, pour l'évaluation complète.
                let best = legal_moves(&after)
                    .into_iter()
                    .filter(|r| r.kind == MoveKind::Attack && r.to == m.to)
                    .map(|r| (apply_move(&after, &r), after.piece(r.piece).kind))
                    .filter(|(x, _)| !x.over)
                    .max_by_key(|(x, _)| full.evaluate(x, victim));
                let Some((end, retaker)) = best else { continue };
                let delta = |e: &Eval| (e.evaluate(&end, victim) - e.evaluate(s, victim)) as f64;
                let total = delta(&full);
                sums[0] += total;
                for (g, e) in parts.iter().enumerate() {
                    sums[1 + g] += total - delta(e);
                }
                let key = format!("{} prend S, {} reprend", name(attacker), name(retaker));
                let entry = by_pair.entry(key).or_default();
                entry.0 += 1;
                entry.1 += total;
                *n += 1;
            }
        }
    }
}

fn report(sums: &[f64; 1 + GROUPS.len()], by_pair: &Pairs, n: u32) {
    let avg = |x: f64| x / n.max(1) as f64;
    println!("{n} échanges (prise d'une Sentinelle puis reprise sur la même case)");
    println!("variation moyenne de l'évaluation pour le camp qui perd sa Sentinelle et reprend :");
    println!("  {:<36} {:>8.1}", "total", avg(sums[0]));
    let mut rest = sums[0];
    for (g, name) in GROUPS.iter().enumerate() {
        println!("  {name:<36} {:>8.1}", avg(sums[1 + g]));
        rest -= sums[1 + g];
    }
    println!(
        "  {:<36} {:>8.1}",
        "reste (autres termes, non-linéarité)",
        avg(rest)
    );
    println!("\npar type de pièces :");
    for (key, &(count, total)) in by_pair {
        println!(
            "  {key:<24} {count:>6} échanges, variation moyenne {:>8.1}",
            total / count as f64
        );
    }
}
