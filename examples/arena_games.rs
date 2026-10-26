//! Parties réelles du site, rejouées avec notre moteur (relief exact de chaque partie).
//!
//! ```text
//! python3 scripts/arena_export.py data/arena/export.json > games.txt
//! cargo run --release --example arena_games -- games.txt                    # résumé, contrôle des règles
//! cargo run --release --example arena_games -- games.txt --follow 3500      # Piolet rejoue ses coups
//! cargo run --release --example arena_games -- games.txt --audit 20000 [--only ID] [--worst 4]
//! ```
//!
//! `--audit N` : pour chaque coup de Piolet, recherche à N nœuds dans la position (meilleur coup et
//! son score), puis dans la position après le coup joué (score du coup joué, vu du même camp).
//! L'écart est la perte estimée du coup joué ; on affiche les pires coups de chaque partie.

use std::cell::Cell;

use piolet::eval::Eval;
use piolet::search::{Config, Piolet};
use traversal_bot::core::bot::WIN;
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

fn name(sq: u8) -> String {
    format!("{}{}", (b'a' + sq % 10) as char, sq / 10 + 1)
}

fn show(s: &State, m: &Move) -> String {
    if m.kind == MoveKind::Pass {
        return "passe".into();
    }
    let p = s.piece(m.piece);
    let k = match p.kind {
        Kind::Explorer => 'E',
        Kind::Vanguard => 'V',
        Kind::Sentinel => 'S',
    };
    match m.kind {
        MoveKind::Exit => format!("{k}{}↑", name(p.sq)),
        MoveKind::Attack => format!("{k}{}x{}", name(p.sq), name(m.to)),
        _ => format!("{k}{}-{}", name(p.sq), name(m.to)),
    }
}

struct Game {
    id: String,
    side: u8,
    result: String,
    reason: String,
    opponent: String,
    mode: String,
    /// Positions avant chaque coup, et le coup joué.
    plies: Vec<(State, Move)>,
    end: State,
}

fn parse(line: &str) -> Result<Game, String> {
    let f: Vec<&str> = line.split('|').collect();
    let mut heights = [0u8; 100];
    for (h, v) in heights.iter_mut().zip(f[6].split(',')) {
        *h = v.parse().map_err(|_| "hauteur")?;
    }
    let mut s = new_game(heights);
    let mut plies = Vec::new();
    for (i, t) in f[7].split_whitespace().enumerate() {
        let p: Vec<&str> = t.split('-').collect();
        let (from, to): (u8, u8) = (p[0].parse().unwrap(), p[1].parse().unwrap());
        let kind = match p[2] {
            "m" => MoveKind::Move,
            "a" => MoveKind::Attack,
            "x" => MoveKind::Exit,
            _ => MoveKind::Pass,
        };
        let m = legal_moves(&s)
            .into_iter()
            .find(|m| {
                m.kind == kind
                    && (kind == MoveKind::Pass
                        || (s.piece(m.piece).sq == from && (kind == MoveKind::Exit || m.to == to)))
            })
            .ok_or_else(|| {
                format!(
                    "{} : demi-coup {} ({t}) illégal pour notre moteur",
                    f[0],
                    i + 1
                )
            })?;
        let next = apply_move(&s, &m);
        plies.push((s, m));
        s = next;
    }
    Ok(Game {
        id: f[0].into(),
        side: f[1].parse().unwrap(),
        result: f[2].into(),
        reason: f[3].into(),
        opponent: f[4].into(),
        mode: f[5].into(),
        plies,
        end: s,
    })
}

fn search(cfg: &Config, s: &State, n: u64) -> (Move, i32, u8) {
    let moves = legal_moves(s);
    let mut bot = Piolet::new(cfg.clone());
    CALLS_LEFT.with(|c| c.set(n.div_ceil(64)));
    let pick = bot.choose(s, &moves, out_of_nodes);
    (moves[pick.index], pick.score, pick.depth)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opt = |n: &str| {
        args.iter()
            .position(|a| a == n)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let text = std::fs::read_to_string(&args[1]).expect("fichier de parties");
    let mut cfg = Config::default();
    if let Some(w) = opt("--w") {
        cfg.weights.apply(&w).expect("--w");
    }
    if let Some(b) = opt("--beam") {
        cfg.beam = b.parse().expect("--beam");
    }
    if args.iter().any(|a| a == "--race") {
        cfg.race = true;
    }
    let only = opt("--only");
    let mut games = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match parse(line) {
            Ok(g) => games.push(g),
            Err(e) => println!("{e}"),
        }
    }
    games.retain(|g| only.as_ref().is_none_or(|o| &g.id == o));
    println!("{} parties rejouées sans désaccord de règles", games.len());
    for g in &games {
        let ok = g.end.over && (g.end.winner == Some(g.side)) == (g.result == "win");
        println!(
            "  {} {} {:<14} {:<5} {:<10} {:>3} demi-coups  {:<10}{}",
            g.id,
            if g.side == 1 { "Rouge" } else { "Bleu " },
            g.opponent,
            g.result,
            g.reason,
            g.plies.len(),
            g.mode,
            if ok || g.result == "draw" {
                ""
            } else {
                "  (fin différente chez nous !)"
            }
        );
    }
    if let Some(n) = opt("--follow").map(|n| n.parse::<u64>().unwrap()) {
        let (mut same, mut total) = (0, 0);
        for g in &games {
            let mut bot = Piolet::new(cfg.clone());
            for (s, played) in g.plies.iter().filter(|(s, _)| s.to_move == g.side) {
                let moves = legal_moves(s);
                CALLS_LEFT.with(|c| c.set(n.div_ceil(64)));
                let m = moves[bot.choose(s, &moves, out_of_nodes).index];
                same +=
                    (m.piece == played.piece && m.to == played.to && m.kind == played.kind) as u32;
                total += 1;
            }
        }
        println!(
            "\nPiolet à {n} nœuds rejoue {same} de ses {total} coups ({:.0} %)",
            100.0 * same as f64 / total as f64
        );
    }
    // `--verdict N` : score de chaque position où Piolet joue (recherche à N nœuds) et coup conseillé.
    // Le passage d'une position tenable à une position perdue désigne le coup fautif.
    if let Some(n) = opt("--verdict").map(|n| n.parse::<u64>().unwrap()) {
        for g in &games {
            println!(
                "\n{} ({} contre {}, {}) :",
                g.id,
                if g.side == 1 { "Rouge" } else { "Bleu" },
                g.opponent,
                g.result
            );
            let all = args.iter().any(|a| a == "--all");
            for (ply, (s, played)) in g.plies.iter().enumerate() {
                if s.to_move != g.side && !all {
                    continue;
                }
                let (best, score, depth) = search(&cfg, s, n);
                let tag = if score > WIN - 1000 {
                    "GAGNÉ"
                } else if score < -WIN + 1000 {
                    "PERDU"
                } else {
                    ""
                };
                let eval = Eval::new(&s.heights, cfg.weights);
                let race: Vec<String> = eval
                    .sentinel_report(s)
                    .iter()
                    .map(|(o, sq, raw, safe)| {
                        format!(
                            "{}{}:{raw}/{safe}",
                            if *o == g.side { "moi " } else { "lui " },
                            name(*sq)
                        )
                    })
                    .collect();
                println!(
                    "  {ply:>3} {:>7} {tag:<6} prof. {depth}  joué {:<10} conseillé {:<10} [{}]",
                    score,
                    show(s, played),
                    show(s, &best),
                    race.join(" ")
                );
            }
        }
    }
    // `--danger P` : carte de la position avant le demi-coup P, pour chaque Sentinelle adverse de
    // Piolet : cases dangereuses (x), cases de sortie en un tour (o), dangereuses et de sortie (X).
    if let Some(ply) = opt("--danger").map(|n| n.parse::<usize>().unwrap()) {
        for g in &games {
            let s = &g.plies[ply].0;
            let eval = Eval::new(&s.heights, cfg.weights);
            for (i, p) in s.pieces().iter().enumerate() {
                if p.kind != Kind::Sentinel || p.owner == g.side {
                    continue;
                }
                let (d, e) = (eval.danger(s, i as u8), eval.exit_mask(p.owner));
                println!("\n{} demi-coup {ply}, Sentinelle adverse en {} (rangée 10 en haut ; minuscules : Rouge) :", g.id, name(p.sq));
                for row in (0..10).rev() {
                    let mut line = format!("{:>2} ", row + 1);
                    for col in 0..10 {
                        let sq = row * 10 + col;
                        let c = match s.at(sq as u8).map(|j| s.piece(j)) {
                            Some(q) => {
                                let k = match q.kind {
                                    Kind::Explorer => 'e',
                                    Kind::Vanguard => 'v',
                                    Kind::Sentinel => 's',
                                };
                                if q.owner == 1 {
                                    k
                                } else {
                                    k.to_ascii_uppercase()
                                }
                            }
                            None => match (d >> sq & 1 == 1, e >> sq & 1 == 1) {
                                (true, true) => 'X',
                                (true, false) => 'x',
                                (false, true) => 'o',
                                _ => '.',
                            },
                        };
                        line += &format!("{c}{} ", s.heights[sq]);
                    }
                    println!("{line}");
                }
                println!("   a  b  c  d  e  f  g  h  i  j");
            }
        }
    }
    if let Some(n) = opt("--audit").map(|n| n.parse::<u64>().unwrap()) {
        let worst: usize = opt("--worst").map_or(4, |w| w.parse().unwrap());
        for g in &games {
            let mut rows = Vec::new();
            for (ply, (s, played)) in g.plies.iter().enumerate() {
                if s.to_move != g.side || legal_moves(s).len() <= 1 {
                    continue;
                }
                let (best, best_score, depth) = search(&cfg, s, n);
                let after = apply_move(s, played);
                let played_score = if after.over {
                    match after.winner {
                        Some(w) if w == g.side => WIN,
                        None => 0,
                        _ => -WIN,
                    }
                } else {
                    -search(&cfg, &after, n).1
                };
                rows.push((
                    best_score.saturating_sub(played_score),
                    ply,
                    show(s, played),
                    played_score,
                    show(s, &best),
                    best_score,
                    depth,
                ));
            }
            let mut sorted = rows.clone();
            sorted.sort_by_key(|r| -r.0);
            println!(
                "\n{} ({} contre {}, {} par {}) — pires coups (recherche à {n} nœuds) :",
                g.id,
                if g.side == 1 { "Rouge" } else { "Bleu" },
                g.opponent,
                g.result,
                g.reason
            );
            for (loss, ply, p, ps, b, bs, d) in sorted.iter().take(worst) {
                println!("  demi-coup {ply:>3} : joué {p:<10} ({ps:>6})  meilleur {b:<10} ({bs:>6}, prof. {d})  perte {loss}");
            }
        }
    }
}
