//! Duel natif Piolet contre Mont Blanc, avec l'analyse des sorties : qui sort le premier,
//! avec quelle pièce, et comment finit la partie ensuite.
//!
//! ```text
//! cargo run --release --example duel -- --games 400 --nodes 3600
//! ```
//!
//! Pas de fuel en natif : Piolet est borné par un budget de nœuds (`--nodes`, ≈ 3 600 en
//! wasm à 50 % du fuel). Mont Blanc (profondeur 4, faisceau 7, compréhension 3) finit
//! toujours sa recherche, comme dans l'arène où sa réserve est de 1/8 du fuel.
//! Cartes et couleurs comme l'arène : partie `g` sur la carte `g / 2`, couleurs alternées.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use piolet::boards;
use piolet::search::{Config, Piolet};
use traversal_bot::core::{apply_move, choose_move, legal_moves, new_game, Kind, Level, MoveKind, Rng};

thread_local! {
    /// Appels restants à `stop` (un appel tous les 64 nœuds) pour le coup en cours.
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

/// Une sortie : qui (`true` = Piolet), quelle pièce, à quel demi-coup.
#[derive(Clone, Copy)]
struct Exit {
    piolet: bool,
    kind: Kind,
    ply: u16,
}

struct Game {
    /// 1 : Piolet gagne, 0 : nulle, -1 : Piolet perd.
    result: i32,
    reason: &'static str,
    exits: Vec<Exit>,
}

fn play(board: [u8; 100], piolet_side: u8, nodes: u64) -> Game {
    let mut bot = Piolet::new(Config::default());
    let mut s = new_game(board);
    let mut exits = Vec::new();
    while !s.over {
        let moves = legal_moves(&s);
        let mv = if s.to_move == piolet_side {
            CALLS_LEFT.with(|c| c.set(nodes.div_ceil(64)));
            moves[bot.choose(&s, &moves, out_of_nodes).index]
        } else {
            choose_move(&s, MONT_BLANC, &mut Rng::new(s.ply as u64)).mv
        };
        if mv.kind == MoveKind::Exit {
            exits.push(Exit { piolet: s.to_move == piolet_side, kind: s.piece(mv.piece).kind, ply: s.ply });
        }
        s = apply_move(&s, &mv);
    }
    let result = match s.winner {
        None => 0,
        Some(w) if w == piolet_side => 1,
        Some(_) => -1,
    };
    Game { result, reason: s.reason.map_or("?", |r| r.name()), exits }
}

fn arg(args: &[String], name: &str, default: u64) -> u64 {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map_or(default, |v| v.parse().expect("nombre"))
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Explorer => "Explorateur",
        Kind::Vanguard => "Avant-garde",
        Kind::Sentinel => "Sentinelle",
    }
}

/// « V / N / D » d'une liste de résultats.
fn wdl(results: &[i32]) -> String {
    let count = |r| results.iter().filter(|&&x| x == r).count();
    format!("{:>3} V / {:>3} N / {:>3} D", count(1), count(0), count(-1))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let games = arg(&args, "--games", 200) as usize;
    let nodes = arg(&args, "--nodes", 3600);
    let threads = arg(&args, "--threads", std::thread::available_parallelism().map_or(4, |n| n.get() as u64)) as usize;
    let boards = boards();

    let next = AtomicUsize::new(0);
    let done = Mutex::new(Vec::with_capacity(games));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let g = next.fetch_add(1, Ordering::Relaxed);
                if g >= games {
                    break;
                }
                let side = if g % 2 == 0 { 1 } else { 2 };
                let game = play(boards[(g / 2) % boards.len()], side, nodes);
                done.lock().unwrap().push(game);
            });
        }
    });
    let all = done.into_inner().unwrap();

    let results: Vec<i32> = all.iter().map(|g| g.result).collect();
    let score = (results.iter().filter(|&&r| r == 1).count() as f64 + 0.5 * results.iter().filter(|&&r| r == 0).count() as f64)
        / all.len() as f64;
    println!("Piolet ({nodes} nœuds) contre Mont Blanc : {} parties", all.len());
    println!("  total                         {}  — score {:.1} %", wdl(&results), 100.0 * score);

    let mut reasons: BTreeMap<&str, Vec<i32>> = BTreeMap::new();
    for g in &all {
        reasons.entry(g.reason).or_default().push(g.result);
    }
    println!("\nFin de partie :");
    for (reason, r) in &reasons {
        println!("  {reason:<29} {}", wdl(r));
    }

    // Le cœur de la mesure : la première sortie, et ce qu'elle a donné.
    let mut first: BTreeMap<(bool, &str), Vec<i32>> = BTreeMap::new();
    let mut gaps: BTreeMap<bool, Vec<u16>> = BTreeMap::new();
    for g in &all {
        let Some(e) = g.exits.first() else {
            first.entry((false, "(aucune sortie)")).or_default().push(g.result);
            continue;
        };
        first.entry((e.piolet, kind_name(e.kind))).or_default().push(g.result);
        if let Some(e2) = g.exits.get(1) {
            gaps.entry(e.piolet).or_default().push(e2.ply - e.ply);
        }
    }
    println!("\nPremière sortie (résultat pour Piolet) :");
    for ((piolet, kind), r) in &first {
        let who = if *kind == "(aucune sortie)" { "" } else if *piolet { "Piolet  " } else { "MontBlanc" };
        println!("  {who:<9} {kind:<19} {}", wdl(r));
    }

    // Qui marque la deuxième sortie, selon qui a fait la première (hors Sentinelle).
    println!("\nAprès une première sortie hors Sentinelle, la deuxième sortie :");
    for piolet_first in [true, false] {
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for g in all.iter().filter(|g| g.exits.first().is_some_and(|e| e.piolet == piolet_first && e.kind != Kind::Sentinel)) {
            let key = match g.exits.get(1) {
                Some(e) => format!("{} ({})", if e.piolet { "Piolet" } else { "Mont Blanc" }, kind_name(e.kind)),
                None => "aucune (limite de coups)".into(),
            };
            *by.entry(key).or_default() += 1;
        }
        let who = if piolet_first { "Piolet" } else { "Mont Blanc" };
        println!("  {who} sort le premier :");
        for (k, n) in by {
            println!("    {k:<30} {n}");
        }
        if let Some(g) = gaps.get(&piolet_first) {
            let mean = g.iter().map(|&x| x as f64).sum::<f64>() / g.len() as f64;
            println!("    écart moyen entre les deux sorties : {mean:.1} demi-coups");
        }
    }
}
