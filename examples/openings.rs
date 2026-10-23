//! Profil de course de chaque carte au départ : tours exacts (`exact_turns`) de la pièce la
//! plus rapide de chaque type, par camp. Une ligne par carte, pour l'analyse stratégique.
//!
//! ```text
//! cargo run --release --example openings > openings.txt
//! # carte  R:E V S  B:E V S
//! ```

use piolet::boards;
use piolet::eval::exact_turns;
use traversal_bot::core::{new_game, Kind};

fn main() {
    for (b, heights) in boards().into_iter().enumerate() {
        let s = new_game(heights);
        let mut line = format!("{b:>3}");
        for owner in [1u8, 2] {
            line.push_str(if owner == 1 { "  R:" } else { "  B:" });
            for kind in [Kind::Explorer, Kind::Vanguard, Kind::Sentinel] {
                let table = exact_turns(&heights, owner, kind);
                let best = s
                    .pieces()
                    .iter()
                    .filter(|p| p.owner == owner && p.kind == kind)
                    .map(|p| table[p.sq as usize])
                    .min()
                    .unwrap_or(u8::MAX);
                line.push_str(&format!(" {best}"));
            }
        }
        println!("{line}");
    }
}
