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
pub mod search;

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use search::{Config, Piolet};
use traversal_bot::{bot, fuel_left, log, Turn};

/// Part du fuel de chaque coup qu'on s'autorise à dépenser (le reste est une marge : un coup
/// qui épuise le fuel fait perdre la partie).
const SPEND_NUM: u64 = 1;
const SPEND_DEN: u64 = 2;

/// Fuel en deçà duquel on arrête de chercher (fixé à chaque coup).
static RESERVE: AtomicU64 = AtomicU64::new(0);

fn out_of_fuel() -> bool {
    fuel_left() < RESERVE.load(Ordering::Relaxed)
}

thread_local! {
    // La mémoire du module persiste d'un coup à l'autre : la table de transposition aussi.
    static BOT: RefCell<Option<Piolet>> = const { RefCell::new(None) };
}

fn choose(turn: &Turn) -> usize {
    RESERVE.store(turn.fuel - turn.fuel * SPEND_NUM / SPEND_DEN, Ordering::Relaxed);
    let pick = BOT.with(|b| {
        let mut b = b.borrow_mut();
        let bot = b.get_or_insert_with(|| Piolet::new(Config::default()));
        bot.choose(&turn.state, &turn.moves, out_of_fuel)
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
