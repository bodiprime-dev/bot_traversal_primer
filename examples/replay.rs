//! Rejouer une partie de l'arène (notation du site) et analyser un coup de Piolet.
//!
//! ```text
//! cargo run --release --example replay -- "Ve1-f3 Sd10-d8 …" [--board N] [--ply P] [--nodes 5000,20000]
//! ```
//!
//! Notation du site : type (E, V, S), case de départ, case d'arrivée (colonnes a à j, rangées 1 à
//! 10 ; Rouge part des rangées basses). Une case d'arrivée illisible ou absente désigne une sortie.
//!
//! Sans `--board`, la carte est retrouvée : on garde les cartes sur lesquelles toute la suite de
//! coups est légale (le relief décide de la portée des coups, la suite suffit souvent à trancher).
//!
//! `--ply P` : position avant le demi-coup P (0 = premier coup). Piolet y cherche à chaque budget
//! de `--nodes` ; on affiche les scores des coups à la racine (le premier est exact, les autres
//! sont des bornes de fenêtre nulle) et le coup réellement joué.

use std::cell::Cell;

use piolet::boards;
use piolet::search::{Config, Piolet};
use traversal_bot::core::{apply_move, legal_moves, new_game, Kind, Move, MoveKind, State};

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

fn square(s: &str) -> Option<u8> {
    let mut c = s.chars();
    let col = c.next()? as i32 - 'a' as i32;
    let row: i32 = c.as_str().parse().ok()?;
    ((0..10).contains(&col) && (1..=10).contains(&row)).then(|| ((row - 1) * 10 + col) as u8)
}

fn name(sq: u8) -> String {
    format!("{}{}", (b'a' + sq % 10) as char, sq / 10 + 1)
}

fn letter(k: Kind) -> char {
    match k {
        Kind::Explorer => 'E',
        Kind::Vanguard => 'V',
        Kind::Sentinel => 'S',
    }
}

fn show(s: &State, m: &Move) -> String {
    if m.kind == MoveKind::Pass {
        return "passe".into();
    }
    let p = s.piece(m.piece);
    let to = if m.kind == MoveKind::Exit {
        "sortie".into()
    } else {
        name(m.to)
    };
    let x = if m.kind == MoveKind::Attack { "x" } else { "-" };
    format!("{}{}{x}{to}", letter(p.kind), name(p.sq))
}

/// Le coup légal correspondant à la notation, s'il existe.
fn find(s: &State, token: &str) -> Option<Move> {
    let kind = match token.chars().next()? {
        'E' => Kind::Explorer,
        'V' => Kind::Vanguard,
        'S' => Kind::Sentinel,
        _ => return None,
    };
    let rest = &token[1..];
    let (from, to) = rest.split_once(['-', 'x']).unwrap_or((rest, ""));
    let (from, to) = (square(from)?, square(to));
    legal_moves(s).into_iter().find(|m| {
        m.kind != MoveKind::Pass && {
            let p = s.piece(m.piece);
            p.sq == from
                && p.kind == kind
                && p.owner == s.to_move
                && m.to == to.unwrap_or(m.to)
                && (to.is_some() || m.kind == MoveKind::Exit)
        }
    })
}

/// La suite rejouée sur cette carte (positions avant chaque coup), ou l'indice du premier coup illégal.
fn replay(heights: [u8; 100], tokens: &[&str]) -> Result<Vec<(State, Move)>, usize> {
    let mut s = new_game(heights);
    let mut out = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let m = find(&s, t).ok_or(i)?;
        let next = apply_move(&s, &m);
        out.push((s, m));
        s = next;
    }
    Ok(out)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opt = |n: &str| {
        args.iter()
            .position(|a| a == n)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    // Contrôle : `--self B` imprime la notation d'une partie au hasard sur la carte B.
    if let Some(b) = opt("--self") {
        let mut s = new_game(boards()[b.parse::<usize>().unwrap()]);
        let mut seed = 12345u64;
        let mut out = Vec::new();
        while !s.over && out.len() < 40 {
            let ms: Vec<Move> = legal_moves(&s)
                .into_iter()
                .filter(|m| m.kind != MoveKind::Pass)
                .collect();
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let m = ms[(seed >> 33) as usize % ms.len()];
            out.push(show(&s, &m).replace("sortie", ""));
            s = apply_move(&s, &m);
        }
        println!("{}", out.join(" "));
        return;
    }
    let moves = args.get(1).expect("suite de coups");
    let tokens: Vec<&str> = moves
        .split_whitespace()
        .filter(|t| t.chars().next().is_some_and(|c| "EVS".contains(c)))
        .collect();
    let all = boards();
    let candidates: Vec<usize> = match opt("--board") {
        Some(b) => vec![b.parse().expect("--board")],
        None => (0..all.len()).collect(),
    };
    let mut found = Vec::new();
    let mut best = (0, 0);
    for &b in &candidates {
        match replay(all[b], &tokens) {
            Ok(game) => found.push((b, game)),
            Err(i) => best = best.max((i, b)),
        }
    }
    if found.is_empty() {
        println!("aucune carte ne rend toute la suite légale (au mieux carte {} : coup {} « {} » illégal)", best.1, best.0, tokens[best.0]);
        return;
    }
    println!(
        "{} coups ; cartes compatibles : {:?}",
        tokens.len(),
        found.iter().map(|(b, _)| b).collect::<Vec<_>>()
    );
    let Some(ply) = opt("--ply").map(|p| p.parse::<usize>().expect("--ply")) else {
        return;
    };
    let budgets: Vec<u64> = opt("--nodes")
        .unwrap_or("5000,20000".into())
        .split(',')
        .map(|n| n.parse().expect("--nodes"))
        .collect();
    for (b, game) in &found {
        let (s, played) = &game[ply];
        println!(
            "\ncarte {b}, demi-coup {ply}, {} au trait ; coup joué : {}",
            if s.to_move == 1 { "Rouge" } else { "Bleu" },
            show(s, played)
        );
        for p in s.pieces() {
            print!(
                "{}{}{} ",
                if p.owner == 1 { 'r' } else { 'b' },
                letter(p.kind),
                name(p.sq)
            );
        }
        println!();
        let moves = legal_moves(s);
        for &n in &budgets {
            let mut bot = Piolet::new(Config::default());
            CALLS_LEFT.with(|c| c.set(n.div_ceil(64)));
            let pick = bot.choose(s, &moves, out_of_nodes);
            let mut scores: Vec<(usize, i32)> = bot.root_scores().to_vec();
            scores.sort_by_key(|&(_, v)| -v);
            println!(
                "  {n} nœuds : profondeur {}, choix {} ({})",
                pick.depth,
                show(s, &moves[pick.index]),
                pick.score
            );
            for (i, v) in scores.iter().take(8) {
                let mark = if moves[*i].piece == played.piece
                    && moves[*i].to == played.to
                    && moves[*i].kind == played.kind
                {
                    "  <- joué"
                } else {
                    ""
                };
                println!("    {:<12} {v:>8}{mark}", show(s, &moves[*i]));
            }
        }
    }
}
