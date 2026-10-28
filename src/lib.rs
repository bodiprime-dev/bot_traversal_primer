//! Piolet : bot de l'arène Traversal.
//!
//! Recherche alpha-beta (voir `search.rs`) dont la durée est bornée par le fuel : à chaque
//! coup, on s'autorise `SPEND` du fuel accordé, puis on joue le coup de la dernière
//! itération complète.
//!
//! ```text
//! cargo build --release --target wasm32-unknown-unknown
//! # -> target/wasm32-unknown-unknown/release/piolet.wasm
//! ```

pub mod eval;
pub mod nn;
pub mod search;
pub mod tactics;
pub mod tempo;

/// Les 200 cartes officielles (`data/boards.json`, copie de celle du SDK v0.2.0), pour les
/// tests et les duels natifs. Inutilisée par le bot : absente du `.wasm`.
pub fn boards() -> Vec<[u8; traversal_bot::core::config::SQUARES]> {
    include_str!("../data/boards.json")
        .split("],[")
        .map(|b| {
            let v: Vec<u8> = b
                .trim_matches(|c| c == '[' || c == ']' || c == '\n')
                .split(',')
                .map(|x| x.trim().parse().expect("hauteur"))
                .collect();
            v.try_into().expect("100 cases")
        })
        .collect()
}

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use search::{Config, Piolet};
use traversal_bot::{bot, fuel_left, log, Turn};

/// Part du fuel de chaque coup qu'on s'autorise à dépenser, en pourcentage (le reste est une
/// marge : un coup qui épuise le fuel fait perdre la partie). Le fuel non dépensé est perdu (il
/// est remis à neuf à chaque coup). Réglable à la compilation : `PIOLET_SPEND=80`.
const SPEND_PERCENT: u64 = match option_env!("PIOLET_SPEND") {
    Some(s) => parse_percent(s),
    None => 50,
};

fn parse_u64(s: &str) -> u64 {
    s.parse().expect("PIOLET_NODES : un entier attendu")
}

const fn parse_percent(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0;
    let mut v = 0u64;
    while i < b.len() {
        assert!(b[i].is_ascii_digit(), "PIOLET_SPEND : un entier attendu");
        v = v * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    assert!(v >= 10 && v <= 95, "PIOLET_SPEND : entre 10 et 95");
    v
}

/// Fuel en deçà duquel on arrête de chercher (fixé à chaque coup).
static RESERVE: AtomicU64 = AtomicU64::new(0);

fn out_of_fuel() -> bool {
    fuel_left() < RESERVE.load(Ordering::Relaxed)
}

/// Mesure du coût (pas pour l'arène) : `PIOLET_NODES=3000` arrête la recherche à un nombre de
/// nœuds fixe, comme les duels natifs. Les décisions ne dépendent plus du fuel : deux versions
/// aux décisions identiques jouent les mêmes parties, et l'écart de fuel est leur seul coût.
const NODES: Option<&str> = option_env!("PIOLET_NODES");
static CALLS_LEFT: AtomicU64 = AtomicU64::new(0);

fn out_of_nodes() -> bool {
    let left = CALLS_LEFT.load(Ordering::Relaxed).saturating_sub(1);
    CALLS_LEFT.store(left, Ordering::Relaxed);
    left == 0 || fuel_left() < 100_000_000
}

thread_local! {
    // La mémoire du module persiste d'un coup à l'autre : la table de transposition aussi.
    static BOT: RefCell<Option<Piolet>> = const { RefCell::new(None) };
}

/// Réglages du bot. Des poids peuvent être fixés à la compilation, sans toucher au code :
/// `PIOLET_W="value_e=18,tempo_race=30" cargo build --release --target wasm32-unknown-unknown`.
fn config() -> Config {
    let mut cfg = Config::default();
    if let Some(w) = option_env!("PIOLET_W") {
        cfg.weights.apply(w).expect("PIOLET_W");
    }
    cfg
}

fn choose(turn: &Turn) -> usize {
    RESERVE.store(
        turn.fuel - turn.fuel / 100 * SPEND_PERCENT,
        Ordering::Relaxed,
    );
    let pick = BOT.with(|b| {
        let mut b = b.borrow_mut();
        let bot = b.get_or_insert_with(|| Piolet::new(config()));
        match NODES {
            Some(n) => {
                CALLS_LEFT.store(parse_u64(n).div_ceil(64), Ordering::Relaxed);
                bot.choose(&turn.state, &turn.moves, out_of_nodes)
            }
            None => bot.choose(&turn.state, &turn.moves, out_of_fuel),
        }
    });
    log(&format!(
        "demi-coup {} : profondeur {}, {} nœuds, score {}, fuel restant {} M",
        turn.state.ply,
        pick.depth,
        pick.nodes,
        pick.score,
        fuel_left() / 1_000_000
    ));
    pick.index.min(turn.moves.len().saturating_sub(1))
}

bot!(choose);
