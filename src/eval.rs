//! Évaluation de Piolet, paramétrée par des poids (`Weights`).
//!
//! Point de départ : portage fidèle de `Ctx::evaluate` du SDK (niveau de compréhension 3),
//! ses constantes devenant les champs de `Weights`. Avec `Weights::default()`, le score est
//! identique au bit près à celui du SDK (voir le test `same_as_sdk`) : chaque terme ajouté
//! ensuite se mesure contre cette base.
//!
//! Termes (du point de vue de `side`) :
//!  - écart de points marqués ;
//!  - pour chaque pièce : valeur selon sa Force + avancée (tours restants pour sortir) ;
//!  - bonus de Sentinelle proche de la sortie tant que personne n'est sorti ;
//!  - course des Sentinelles (R8.2).

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use traversal_bot::core::bot::{exit_costs, WIN};
use traversal_bot::core::config::{SIZE, SQUARES};
use traversal_bot::core::movegen::{neighbors_of, piece_moves, step_cost};
use traversal_bot::core::state::MAX_PIECES;
use traversal_bot::core::{apply_move, Kind, Move, MoveKind, Piece, State};

/// Poids de l'évaluation. `Weights::sdk()` reproduit l'évaluation du SDK (`Tuning::default()`
/// et les constantes en dur de `Ctx::evaluate`) ; `Weights::default()` y ajoute la course à
/// la sortie décisive (`start_race`, `exit_race`).
#[derive(Clone, Copy, Debug)]
pub struct Weights {
    /// Course des Sentinelles quand personne n'est sorti (voir `Eval::start_race`) : valeur
    /// d'une course gagnée à coup sûr. 0 : celle du SDK (`sentinel_race`).
    pub start_race: i32,
    /// Course à la sortie décisive après une première sortie (voir `Eval::exit_race`) : « ne
    /// pas sortir sans plan ». 0 : désactivée (le SDK compte l'écart de points brut).
    pub exit_race: i32,
    /// Incertitude (en demi-coups) d'une course : `race_spread + d / race_spread_per`, où `d`
    /// est le délai de l'événement le plus proche. Plus la course est lointaine, plus une
    /// même marge compte peu.
    pub race_spread: i32,
    pub race_spread_per: i32,
    /// Valeur d'un point marqué (sortie).
    pub score: i32,
    /// Valeur d'une pièce sur le plateau, par type : [Explorateur, Avant-garde, Sentinelle].
    pub value: [i32; 3],
    /// Avancée : une pièce à `t` tours de la sortie vaut `(progress_horizon - t) * progress[type]`.
    pub progress_horizon: i32,
    pub progress: [i32; 3],
    /// Sentinelle proche de la sortie (personne n'est sorti) : `(horizon - t)² * sentinel_sq`.
    pub sentinel_horizon: i32,
    pub sentinel_sq: i32,
    /// Course des Sentinelles gagnée / perdue, et à partir de combien de tours elle compte.
    pub sentinel_race: i32,
    pub sentinel_race_horizon: i32,
    /// Distances « avec obstacles » : traverser une alliée coûte `ally`, abattre une pièce
    /// adverse (hors Sentinelle, qui reste un mur) coûte `soft` ; le détour est plafonné à
    /// `detour` ; `blend` (en %) dose distance avec obstacles / sur le relief seul.
    pub ally: u16,
    pub soft: u16,
    pub detour: u16,
    pub blend: i32,
    /// Piège : une pièce dont la défense (Force + 1 par allié voisin, R7) atteint ce seuil est
    /// un mur pour l'adversaire, comme une Sentinelle (une Sentinelle seule attaque à 3 : une
    /// défense de 4 l'arrête). 0 : seules les Sentinelles sont des murs (SDK).
    pub wall_defense: i32,
    /// Attaque : par point de Force au contact d'une Sentinelle adverse (ces pièces s'ajoutent
    /// à l'attaque contre elle, R7), plafonné à sa défense : au-delà, ça ne sert plus.
    pub sentinel_pressure: i32,
    /// Valeur d'une Sentinelle encore sur le plateau, en plus de sa Force : prendre la leur,
    /// protéger la nôtre.
    pub sentinel_alive: i32,
    /// Tours pour sortir calculés exactement (`exact_turns` : la marche ne se coupe pas entre
    /// deux tours, coups spéciaux de l'Explorateur) au lieu de `ceil(coût / Mouvement)`. Le
    /// blocage par les pièces s'y ajoute en tours supplémentaires.
    pub exact_turns: bool,
    /// Menace réelle : pénalité, pour le camp au trait, par Sentinelle que l'adversaire peut
    /// capturer à son prochain coup (règle de combat du moteur : soutiens, hauteur, charge,
    /// coups spéciaux). La recherche de calme suppose que le camp au trait peut « ne rien
    /// faire » ; ici, il doit dépenser un coup pour sauver sa Sentinelle, ou la perdre.
    /// (Une Sentinelle capturable par le camp au trait, elle, est vue : la recherche de calme
    /// joue la prise.)
    pub sentinel_threat: i32,
    /// Course en temps (personne n'est sorti) : par demi-coup d'avance de notre meilleure
    /// Sentinelle sur la meilleure adverse (tours restants, blocages compris, trait inclus),
    /// borné à ± `tempo_cap` demi-coups. Linéaire : chaque temps perdu ou gagné compte, partout
    /// (la course du SDK, `sentinel_race`, est un tout ou rien à 5 tours de la sortie).
    pub tempo_race: i32,
    pub tempo_cap: i32,
    /// Couverture : par point de Force d'une pièce qu'une alliée peut reprendre (attaquer sa
    /// case au coup suivant) si elle est capturée. Valorise la deuxième ligne derrière le
    /// convoi, l'Explorateur surtout (allonge, diagonales, bonds).
    pub cover: i32,
    /// Puissance de la couverture : 0, binaire (une alliée peut attaquer la case). Sinon la
    /// couverture vaut `cover × Force × min(puissance, cover_cap) / cover_cap`, où la puissance
    /// est la meilleure attaque de reprise (R7.3) : Force du repreneur, ±1 selon la hauteur, +
    /// Force de nos autres pièces voisines de la case. Une puissance de 3 reprend même une
    /// Sentinelle seule.
    pub cover_cap: i32,
    /// Piolet-NN : si non nul, l'évaluation (hors fin de partie) est celle du réseau
    /// (`nn::net`) : `nn × logit(probabilité de gagner)`. ~270 : l'échelle de l'évaluation
    /// actuelle (pente de sa régression logistique sur les données).
    pub nn: i32,
    /// Avec le réseau : ajouter `exit_race` après une première sortie (une règle, pas une
    /// appréciation). 0 / 1.
    pub nn_race: i32,
    /// Piolet-NN en correction : si non nul, l'évaluation vaut l'évaluation habituelle plus
    /// `nn_resid × sortie du réseau` (en logit), le réseau étant entraîné par `nn/rank.py
    /// --residual` à corriger l'évaluation actuelle là où une recherche profonde la contredit.
    /// 270 : l'échelle de l'entraînement (points par unité de logit).
    pub nn_resid: i32,
    /// Distance SÛRE des Sentinelles (0 : coupé ; n : allongement plafonné à n tours) : tours
    /// pour sortir en ne s'arrêtant que sur des cases où aucune pièce adverse ne peut la capturer
    /// au coup suivant (règle de combat : hauteur, charge de l'Avant-garde à coût exactement 4,
    /// soutiens). Remplace la distance brute de la Sentinelle si elle est plus longue, d'au plus
    /// `safe_race` tours (les pièces adverses bougent aussi : un chemin fermé maintenant ne l'est
    /// pas pour toujours). Voit, dans les deux sens, une Avant-garde bien placée qui interdit une
    /// zone et force un détour.
    pub safe_race: i32,
}

impl Default for Weights {
    fn default() -> Weights {
        // `start_race` reste à 0 : en A/B (400 parties contre Mont Blanc), elle dégrade la course
        // des Sentinelles (voir HANDOFF.md). `exit_race` seule : 49,9 % -> 55,8 %.
        // `tempo_race` 25 : 254 parties améliorées / 204 dégradées sur 3 budgets (z ≈ +2,3),
        // surtout en Bleu ; arène wasm 70,6 % -> 72,5 %.
        Weights {
            start_race: 0,
            exit_race: 400,
            race_spread: 3,
            race_spread_per: 4,
            tempo_race: 25,
            ..Weights::sdk()
        }
    }
}

impl Weights {
    /// Noms des poids réglables par `set` / `get` (réglage automatique, options du duel).
    pub const NAMES: [&'static str; 20] = [
        "value_e",
        "value_v",
        "value_s",
        "progress_e",
        "progress_v",
        "progress_s",
        "progress_horizon",
        "sentinel_horizon",
        "sentinel_sq",
        "sentinel_race",
        "start_race",
        "exit_race",
        "tempo_race",
        "tempo_cap",
        "cover",
        "cover_cap",
        "nn",
        "nn_race",
        "nn_resid",
        "safe_race",
    ];

    fn slot(&mut self, name: &str) -> Option<&mut i32> {
        Some(match name {
            "value_e" => &mut self.value[0],
            "value_v" => &mut self.value[1],
            "value_s" => &mut self.value[2],
            "progress_e" => &mut self.progress[0],
            "progress_v" => &mut self.progress[1],
            "progress_s" => &mut self.progress[2],
            "progress_horizon" => &mut self.progress_horizon,
            "sentinel_horizon" => &mut self.sentinel_horizon,
            "sentinel_sq" => &mut self.sentinel_sq,
            "sentinel_race" => &mut self.sentinel_race,
            "start_race" => &mut self.start_race,
            "exit_race" => &mut self.exit_race,
            "tempo_race" => &mut self.tempo_race,
            "tempo_cap" => &mut self.tempo_cap,
            "cover" => &mut self.cover,
            "cover_cap" => &mut self.cover_cap,
            "nn" => &mut self.nn,
            "nn_race" => &mut self.nn_race,
            "nn_resid" => &mut self.nn_resid,
            "safe_race" => &mut self.safe_race,
            _ => return None,
        })
    }

    /// Fixe un poids par son nom (voir `NAMES`).
    pub fn set(&mut self, name: &str, value: i32) -> Result<(), String> {
        *self
            .slot(name)
            .ok_or_else(|| format!("poids inconnu : {name}"))? = value;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<i32> {
        self.clone().slot(name).map(|v| *v)
    }

    /// Applique des réglages « nom=valeur,nom=valeur ».
    pub fn apply(&mut self, overrides: &str) -> Result<(), String> {
        for kv in overrides.split(',').filter(|s| !s.trim().is_empty()) {
            let (k, v) = kv
                .split_once('=')
                .ok_or_else(|| format!("attendu nom=valeur : {kv}"))?;
            self.set(
                k.trim(),
                v.trim()
                    .parse()
                    .map_err(|_| format!("valeur invalide : {kv}"))?,
            )?;
        }
        Ok(())
    }

    /// L'évaluation du SDK, au bit près (test `same_as_sdk`).
    pub fn sdk() -> Weights {
        Weights {
            start_race: 0,
            exit_race: 0,
            race_spread: 3,
            race_spread_per: 4,
            score: 100,
            // SDK : 14 × Force, et 3 + 2 × Force par tour d'avance.
            value: [14, 28, 42],
            progress_horizon: 7,
            progress: [5, 7, 9],
            sentinel_horizon: 6,
            sentinel_sq: 5,
            sentinel_race: 150,
            sentinel_race_horizon: 5,
            ally: 2,
            soft: 2,
            detour: 6,
            blend: 100,
            wall_defense: 0,
            sentinel_pressure: 0,
            sentinel_alive: 0,
            exact_turns: false,
            sentinel_threat: 0,
            tempo_race: 0,
            tempo_cap: 6,
            cover: 0,
            cover_cap: 0,
            nn: 0,
            nn_race: 0,
            nn_resid: 0,
            safe_race: 0,
        }
    }
}

/// Coût de marche minimal depuis `from` vers chaque case, borné à `budget` (`u8::MAX` au-delà) :
/// ce qu'une pièce atteint en un tour sur le relief seul (R4).
/// Cases qu'une pièce `kind` attaque en un tour depuis chaque case, sur le relief seul : la
/// dernière marche entre sur la case attaquée, donc coût de marche ≤ Mouvement ; plus les
/// diagonales et bonds de l'Explorateur (R5). Champ de bits : bit `c` = case `c`.
pub fn attack_reach(heights: &[u8; SQUARES], kind: Kind) -> [u128; SQUARES] {
    let mut out = [0u128; SQUARES];
    for sq in 0..SQUARES as u8 {
        let dist = walk(heights, sq, kind.movement());
        let mut bits = 0u128;
        for (c, &d) in dist.iter().enumerate() {
            if d != u8::MAX && c != sq as usize {
                bits |= 1 << c;
            }
        }
        if kind == Kind::Explorer {
            let (x, y) = ((sq as usize % SIZE) as i32, (sq as usize / SIZE) as i32);
            for (dx, dy) in [
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
                (0, 2),
                (2, 0),
                (0, -2),
                (-2, 0),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if (0..SIZE as i32).contains(&nx) && (0..SIZE as i32).contains(&ny) {
                    bits |= 1 << (ny * SIZE as i32 + nx);
                }
            }
        }
        out[sq as usize] = bits;
    }
    out
}

fn walk(heights: &[u8; SQUARES], from: u8, budget: u8) -> [u8; SQUARES] {
    let mut dist = [u8::MAX; SQUARES];
    dist[from as usize] = 0;
    // Coûts ≤ 5 : une file par coût suffit (comme `movegen::reach`).
    let mut buckets: [Vec<u8>; 6] = Default::default();
    buckets[0].push(from);
    for d in 0..=budget as usize {
        let mut k = 0;
        while k < buckets[d].len() {
            let cur = buckets[d][k];
            k += 1;
            if dist[cur as usize] as usize != d {
                continue;
            }
            for &nb in neighbors_of(cur) {
                let nd = d + step_cost(heights, cur, nb) as usize;
                if nd <= budget as usize && nd < dist[nb as usize] as usize {
                    dist[nb as usize] = nd as u8;
                    buckets[nd].push(nb);
                }
            }
        }
    }
    dist
}

/// Tours minimaux pour sortir depuis chaque case, pour une pièce `kind` de `owner`, sur le
/// relief seul : exacts, là où `ceil(coût / Mouvement)` se trompe (une marche ne se coupe pas
/// entre deux tours, et l'Explorateur a ses diagonales et ses bonds qui ignorent la hauteur).
///
/// - 1 tour si la pièce atteint la dernière rangée en marchant avec au moins 1 point de
///   Mouvement restant pour sortir (R6) ;
/// - sinon 1 + le minimum sur les cases atteignables en un tour (marche de coût ≤ Mouvement,
///   ou coup spécial de l'Explorateur, R5).
///
/// `u8::MAX` : sortie impossible.
pub fn exact_turns(heights: &[u8; SQUARES], owner: u8, kind: Kind) -> [u8; SQUARES] {
    let budget = kind.movement();
    let exit_row = if owner == 1 { SIZE - 1 } else { 0 };
    let mut turns = [u8::MAX; SQUARES];
    let mut next: Vec<Vec<u8>> = Vec::with_capacity(SQUARES);
    for sq in 0..SQUARES as u8 {
        let dist = walk(heights, sq, budget);
        if (0..SIZE).any(|x| (dist[exit_row * SIZE + x] as u16) < budget as u16) {
            turns[sq as usize] = 1;
        }
        let mut out: Vec<u8> = (0..SQUARES as u8)
            .filter(|&s| s != sq && dist[s as usize] != u8::MAX)
            .collect();
        if kind == Kind::Explorer {
            let (x, y) = ((sq as usize % SIZE) as i32, (sq as usize / SIZE) as i32);
            for (dx, dy) in [
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
                (0, 2),
                (2, 0),
                (0, -2),
                (-2, 0),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if (0..SIZE as i32).contains(&nx) && (0..SIZE as i32).contains(&ny) {
                    out.push((ny * SIZE as i32 + nx) as u8);
                }
            }
        }
        next.push(out);
    }
    // Relaxation jusqu'au point fixe (au plus une dizaine de passes sur 10 × 10).
    let mut changed = true;
    while changed {
        changed = false;
        for sq in 0..SQUARES {
            let best = next[sq]
                .iter()
                .map(|&s| turns[s as usize])
                .min()
                .unwrap_or(u8::MAX);
            if best != u8::MAX && best + 1 < turns[sq] {
                turns[sq] = best + 1;
                changed = true;
            }
        }
    }
    turns
}

/// Taille de l'anneau de la file à seaux de `blocked_costs` (> coût maximal d'une arête).
const RING: usize = 8;

/// Données fixes pendant un coup : le relief ne change pas, les coûts de sortie « relief
/// seul » se calculent une fois.
pub struct Eval {
    costs: [[u16; SQUARES]; 2],
    /// `exact[camp][type][case]` (types : Explorateur, Avant-garde, Sentinelle), si
    /// `exact_turns`.
    exact: Option<Box<[[[u8; SQUARES]; 3]; 2]>>,
    w: Weights,
    /// Tampon de coups réutilisé (pas d'allocation par évaluation).
    buf: RefCell<Vec<Move>>,
    /// `reach[type][case]` : cases qu'une pièce de ce type attaque en un tour depuis cette case
    /// (relief seul : marche de coût ≤ Mouvement, plus diagonales et bonds de l'Explorateur),
    /// si `cover`.
    reach: Option<Box<[[u128; SQUARES]; 3]>>,
    /// Tables de la distance sûre, si `safe_race`.
    safe: Option<Box<SafeTables>>,
}

/// Précalculs de la distance sûre (le relief ne change pas pendant la partie).
struct SafeTables {
    /// `walk[type][départ][arrivée]` : coût de marche, `u8::MAX` au-delà du Mouvement du type.
    walk: [[[u8; SQUARES]; SQUARES]; 3],
    /// Cases où une Sentinelle peut s'arrêter en un tour (relief seul), par case de départ.
    turn: [u128; SQUARES],
    /// Cases d'où une Sentinelle de chaque camp sort en un tour.
    exit: [u128; 2],
}

impl Eval {
    pub fn new(heights: &[u8; SQUARES], w: Weights) -> Eval {
        let exact = w.exact_turns.then(|| {
            let table = |o| {
                [Kind::Explorer, Kind::Vanguard, Kind::Sentinel].map(|k| exact_turns(heights, o, k))
            };
            Box::new([table(1), table(2)])
        });
        let reach = (w.cover != 0).then(|| {
            Box::new(
                [Kind::Explorer, Kind::Vanguard, Kind::Sentinel].map(|k| attack_reach(heights, k)),
            )
        });
        let safe = (w.safe_race != 0).then(|| {
            let mut walk = [[[u8::MAX; SQUARES]; SQUARES]; 3];
            for (k, kind) in [Kind::Explorer, Kind::Vanguard, Kind::Sentinel]
                .into_iter()
                .enumerate()
            {
                for from in 0..SQUARES {
                    walk[k][from] = self::walk(heights, from as u8, kind.movement());
                }
            }
            let mut turn = [0u128; SQUARES];
            for (from, t) in turn.iter_mut().enumerate() {
                for to in 0..SQUARES {
                    if to != from && walk[2][from][to] != u8::MAX {
                        *t |= 1 << to;
                    }
                }
            }
            let exit = [1u8, 2].map(|o| {
                let e = exact_turns(heights, o, Kind::Sentinel);
                (0..SQUARES)
                    .filter(|&s| e[s] == 1)
                    .fold(0u128, |m, s| m | 1 << s)
            });
            Box::new(SafeTables { walk, turn, exit })
        });
        Eval {
            costs: [exit_costs(heights, 1), exit_costs(heights, 2)],
            exact,
            w,
            buf: RefCell::new(Vec::with_capacity(64)),
            reach,
            safe,
        }
    }

    #[inline]
    fn cost(&self, owner: u8, sq: u8) -> i32 {
        self.costs[owner as usize - 1][sq as usize] as i32
    }

    #[inline]
    fn turns_for(cost: i32, kind: Kind) -> i32 {
        let m = kind.movement() as i32;
        (cost + m - 1) / m
    }

    /// Cases de parade contre `them` : celles d'un plus court chemin (relief seul) de sa
    /// Sentinelle la plus proche de la sortie (à défaut, de sa pièce la plus rapide) ; s'y
    /// placer la ralentit ou la force au détour. Une case est sur un plus court chemin si
    /// (coureur → case) + (case → sortie) = (coureur → sortie).
    pub fn guard_squares(&self, state: &State, them: u8) -> [bool; SQUARES] {
        let mut out = [false; SQUARES];
        let turns = |p: &&Piece| Eval::turns_for(self.cost(p.owner, p.sq), p.kind);
        let theirs = || state.pieces().iter().filter(|p| p.owner == them);
        let Some(runner) = theirs()
            .filter(|p| p.kind == Kind::Sentinel)
            .min_by_key(turns)
            .or_else(|| theirs().min_by_key(turns))
        else {
            return out;
        };
        let mut from = [u16::MAX; SQUARES];
        let mut heap = BinaryHeap::new();
        from[runner.sq as usize] = 0;
        heap.push(Reverse((0u16, runner.sq)));
        while let Some(Reverse((d, cur))) = heap.pop() {
            if d > from[cur as usize] {
                continue;
            }
            for &nb in neighbors_of(cur) {
                let nd = d + step_cost(&state.heights, cur, nb) as u16;
                if nd < from[nb as usize] {
                    from[nb as usize] = nd;
                    heap.push(Reverse((nd, nb)));
                }
            }
        }
        let total = self.cost(them, runner.sq);
        for sq in 0..SQUARES {
            if sq != runner.sq as usize
                && from[sq] != u16::MAX
                && from[sq] as i32 + self.cost(them, sq as u8) == total
            {
                out[sq] = true;
            }
        }
        out
    }

    /// Une pièce adverse peut-elle capturer la pièce sur `target` à son prochain coup ? Coups
    /// du moteur (`piece_moves`), limités aux pièces assez proches : une attaque coûte au
    /// moins la distance de Manhattan (un coup spécial de l'Explorateur, au plus 2 cases).
    pub fn capturable(&self, state: &State, target: Piece) -> bool {
        let (tx, ty) = (
            (target.sq as usize % SIZE) as i32,
            (target.sq as usize / SIZE) as i32,
        );
        let mut buf = self.buf.borrow_mut();
        for (j, q) in state.pieces().iter().enumerate() {
            if q.owner == target.owner {
                continue;
            }
            let (qx, qy) = ((q.sq as usize % SIZE) as i32, (q.sq as usize / SIZE) as i32);
            if (qx - tx).abs() + (qy - ty).abs() > q.kind.movement() as i32 {
                continue;
            }
            buf.clear();
            piece_moves(state, j as u8, &mut buf);
            if buf
                .iter()
                .any(|m| m.kind == MoveKind::Attack && m.to == target.sq)
            {
                return true;
            }
        }
        false
    }

    /// Coup de chasse : un coup calme après lequel une Sentinelle adverse devient capturable
    /// par le camp qui le joue (la pièce déplacée attaque ou vient soutenir une attaque).
    /// Filtre d'abord sur la distance : la destination doit être à portée de la Sentinelle.
    pub fn hunts(&self, state: &State, m: &Move) -> bool {
        if m.kind != MoveKind::Move {
            return false;
        }
        let mover = state.to_move;
        let reach = state.piece(m.piece).kind.movement() as i32;
        let (x, y) = ((m.to as usize % SIZE) as i32, (m.to as usize / SIZE) as i32);
        let near = |p: &Piece| {
            let (px, py) = ((p.sq as usize % SIZE) as i32, (p.sq as usize / SIZE) as i32);
            p.owner != mover && p.kind == Kind::Sentinel && (px - x).abs() + (py - y).abs() <= reach
        };
        if !state.pieces().iter().any(near) {
            return false;
        }
        let child = apply_move(state, m);
        let hunted = child
            .pieces()
            .iter()
            .filter(|p| p.owner != mover && p.kind == Kind::Sentinel)
            .any(|p| self.capturable(&child, *p));
        // Déjà capturable avant le coup : ce n'est pas lui qui crée la menace.
        hunted
            && !state
                .pieces()
                .iter()
                .filter(|p| p.owner != mover && p.kind == Kind::Sentinel)
                .any(|p| self.capturable(state, *p))
    }

    /// Cases où une Sentinelle de `owner` serait capturable par une pièce adverse au coup suivant
    /// (champ de bits). Règle de combat (R7) : Force de l'attaquant, ±1 selon la hauteur de sa
    /// case, +1 de charge pour l'Avant-garde à coût exactement 4, + Force des autres pièces adverses
    /// voisines de la case ; défense : 3 + 1 par allié voisin. Marche sur le relief seul (les pièces
    /// sur le chemin de l'attaquant sont ignorées) ; diagonales et bonds de l'Explorateur compris.
    /// `mover` : indice de la Sentinelle qui se déplace (elle ne se soutient pas elle-même).
    fn danger(&self, state: &State, mover: u8) -> u128 {
        let tables = self.safe.as_ref().expect("safe_race");
        let owner = state.piece(mover).owner;
        let h = &state.heights;
        let mut mask = 0u128;
        for s in 0..SQUARES as u8 {
            if state.at(s).is_some_and(|i| state.piece(i).owner == owner) {
                continue; // case occupée par un allié : on ne s'y arrête pas
            }
            let (mut defense, mut support) = (3i32, 0i32);
            for &nb in neighbors_of(s) {
                if let Some(i) = state.at(nb).filter(|&i| i != mover) {
                    let q = state.piece(i);
                    if q.owner == owner {
                        defense += 1;
                    } else {
                        support += q.kind.strength() as i32;
                    }
                }
            }
            let (sx, sy) = ((s as usize % SIZE) as i32, (s as usize / SIZE) as i32);
            for (qi, q) in state.pieces().iter().enumerate() {
                if q.owner == owner || state.at(s) == Some(qi as u8) {
                    continue;
                }
                let k = q.kind.strength() as usize - 1;
                let d = tables.walk[k][q.sq as usize][s as usize];
                let special = q.kind == Kind::Explorer && {
                    let (qx, qy) = ((q.sq as usize % SIZE) as i32, (q.sq as usize / SIZE) as i32);
                    let (dx, dy) = ((qx - sx).abs(), (qy - sy).abs());
                    (dx == 1 && dy == 1) || (dx + dy == 2 && (dx == 0 || dy == 0))
                };
                if d == u8::MAX && !special {
                    continue;
                }
                let adjacent = neighbors_of(s).contains(&q.sq);
                let height = (h[q.sq as usize] > h[s as usize]) as i32
                    - (h[q.sq as usize] < h[s as usize]) as i32;
                let charge = (q.kind == Kind::Vanguard && d == 4) as i32;
                // Les voisines adverses soutiennent ; l'attaquant lui-même, s'il est voisin, ne se compte pas.
                let attack = q.kind.strength() as i32 + height + charge + support
                    - if adjacent {
                        q.kind.strength() as i32
                    } else {
                        0
                    };
                if attack >= defense {
                    mask |= 1 << s;
                    break;
                }
            }
        }
        mask
    }

    /// Tours pour qu'une Sentinelle sorte en ne s'arrêtant que sur des cases sûres (`danger` :
    /// cases à éviter). Relief seul pour la marche ; on ne s'arrête ni sur un allié ni sur une
    /// Sentinelle adverse (une autre pièce adverse peut être prise). 15 : pas de chemin sûr.
    fn safe_turns(&self, state: &State, p: &Piece, danger: u128) -> i32 {
        let tables = self.safe.as_ref().expect("safe_race");
        let exit = tables.exit[p.owner as usize - 1];
        let mut blocked = danger;
        for q in state.pieces() {
            if q.owner == p.owner || q.kind == Kind::Sentinel {
                blocked |= 1 << q.sq;
            }
        }
        let mut frontier = 1u128 << p.sq;
        let mut seen = frontier;
        for t in 1..15 {
            if frontier & exit != 0 {
                return t;
            }
            let mut next = 0u128;
            let mut f = frontier;
            while f != 0 {
                let s = f.trailing_zeros() as usize;
                f &= f - 1;
                next |= tables.turn[s];
            }
            next &= !seen & !blocked;
            if next == 0 {
                return 15;
            }
            seen |= next;
            frontier = next;
        }
        15
    }

    /// Défense d'une pièce (R7.4) : sa Force + 1 par allié voisin.
    fn defense(state: &State, p: &Piece) -> i32 {
        let allies = neighbors_of(p.sq)
            .iter()
            .filter(|&&nb| {
                state
                    .at(nb)
                    .is_some_and(|j| state.piece(j).owner == p.owner)
            })
            .count();
        p.kind.strength() as i32 + allies as i32
    }

    /// Coût de sortie depuis chaque case pour `owner`, en tenant compte des pièces : une
    /// Sentinelle adverse (ou, avec `wall_defense`, toute pièce adverse assez défendue) est un
    /// mur, les autres pièces adverses se franchissent en les abattant (`soft`), traverser
    /// une alliée coûte `ally`.
    fn blocked_costs(&self, state: &State, owner: u8) -> [u16; SQUARES] {
        let w = &self.w;
        let row = if owner == 1 { SIZE - 1 } else { 0 };
        let other = |sq: u8| {
            state
                .at(sq)
                .map(|i| state.piece(i))
                .filter(|p| p.owner != owner)
        };
        let mut walls = [false; SQUARES];
        for p in state.pieces().iter().filter(|p| p.owner != owner) {
            walls[p.sq as usize] = w.soft == 0
                || p.kind == Kind::Sentinel
                || (w.wall_defense > 0 && Eval::defense(state, p) >= w.wall_defense);
        }
        let wall = |sq: u8| walls[sq as usize];
        let ally = |sq: u8| state.at(sq).is_some_and(|i| state.piece(i).owner == owner);
        // Coût pour ENTRER sur une case depuis une voisine : la marche, plus la pièce à écarter
        // (alliée) ou à abattre (adverse).
        let enter = |cur: u8| {
            if ally(cur) {
                w.ally
            } else if other(cur).is_some() {
                w.soft
            } else {
                0
            }
        };
        let mut dist = [u16::MAX; SQUARES];
        // Arêtes de coût 1 à 3 + max(ally, soft) : file à seaux en anneau (Dial), sans tas ni
        // allocation. Le tableau des distances ne dépend pas de l'ordre de traitement : même
        // résultat que Dijkstra avec tas (test same_as_sdk). Au-delà, repli sur le tas.
        let max_edge = 3 + w.ally.max(w.soft) as usize;
        if max_edge < RING {
            let mut ring = [[0u8; SQUARES]; RING];
            let mut len = [0usize; RING];
            let mut pending = 0usize;
            for x in 0..SIZE {
                let s = (row * SIZE + x) as u8;
                if !wall(s) {
                    dist[s as usize] = 1;
                    ring[1][len[1]] = s;
                    len[1] += 1;
                    pending += 1;
                }
            }
            let mut d = 1usize;
            while pending > 0 {
                let slot = d % RING;
                let mut k = 0;
                while k < len[slot] {
                    let cur = ring[slot][k];
                    k += 1;
                    pending -= 1;
                    if dist[cur as usize] as usize != d {
                        continue; // entrée périmée
                    }
                    let e = enter(cur);
                    for &nb in neighbors_of(cur) {
                        if wall(nb) {
                            continue;
                        }
                        let nd = d + step_cost(&state.heights, nb, cur) as usize + e as usize;
                        if nd < dist[nb as usize] as usize {
                            dist[nb as usize] = nd as u16;
                            let s2 = nd % RING;
                            ring[s2][len[s2]] = nb;
                            len[s2] += 1;
                            pending += 1;
                        }
                    }
                }
                len[slot] = 0;
                d += 1;
            }
            return dist;
        }
        let mut heap = BinaryHeap::new();
        for x in 0..SIZE {
            let s = (row * SIZE + x) as u8;
            if !wall(s) {
                dist[s as usize] = 1;
                heap.push(Reverse((1u16, s)));
            }
        }
        while let Some(Reverse((d, cur))) = heap.pop() {
            if d > dist[cur as usize] {
                continue;
            }
            let e = enter(cur);
            for &nb in neighbors_of(cur) {
                if wall(nb) {
                    continue;
                }
                let nd = d + step_cost(&state.heights, nb, cur) as u16 + e;
                if nd < dist[nb as usize] {
                    dist[nb as usize] = nd;
                    heap.push(Reverse((nd, nb)));
                }
            }
        }
        dist
    }

    /// Tours (au mieux) pour sortir, pour chaque pièce, obstacles compris.
    pub(crate) fn piece_turns(&self, state: &State) -> [i32; MAX_PIECES] {
        let w = &self.w;
        let mut out = [0; MAX_PIECES];
        let blocked = [self.blocked_costs(state, 1), self.blocked_costs(state, 2)];
        for (i, p) in state.pieces().iter().enumerate() {
            let free = self.cost(p.owner, p.sq);
            let with =
                (blocked[p.owner as usize - 1][p.sq as usize] as i32).min(free + w.detour as i32);
            let cost = (free * (100 - w.blend) + with * w.blend + 50) / 100;
            out[i] = match &self.exact {
                None => Eval::turns_for(cost, p.kind),
                Some(exact) => {
                    let k = p.kind.strength() as usize - 1; // Explorateur 0, Avant-garde 1, Sentinelle 2
                    let base = match exact[p.owner as usize - 1][k][p.sq as usize] {
                        u8::MAX => 99,
                        t => t as i32,
                    };
                    // Les pièces sur le chemin : tours en plus par rapport au relief seul.
                    base + (Eval::turns_for(cost, p.kind) - Eval::turns_for(free, p.kind)).max(0)
                }
            };
        }
        out
    }

    /// Évaluation du point de vue de `side`.
    pub fn evaluate(&self, state: &State, side: u8) -> i32 {
        if state.over {
            return match state.winner {
                None => 0,
                Some(w) if w == side => WIN - state.ply as i32,
                Some(_) => -WIN + state.ply as i32,
            };
        }
        if self.w.nn != 0 {
            return self.evaluate_nn(state, side);
        }
        if self.w.nn_resid != 0 {
            // Évaluation habituelle + correction du réseau, toutes deux du point de vue du camp au
            // trait (comme à l'entraînement), puis retournée pour l'autre camp.
            let mover = state.to_move;
            let turns = self.piece_turns(state); // calculé une seule fois pour les deux
            let (sparse, dense) = crate::nn::features_with(self, state, &turns);
            let correction =
                crate::nn::net().logit(&sparse, &dense).clamp(-30.0, 30.0) * self.w.nn_resid as f32;
            let v = self.evaluate_base_with(state, mover, turns) + correction as i32;
            return if side == mover { v } else { -v };
        }
        self.evaluate_base(state, side)
    }

    /// Évaluation par termes (hors réseau), du point de vue de `side`.
    fn evaluate_base(&self, state: &State, side: u8) -> i32 {
        self.evaluate_base_with(state, side, self.piece_turns(state))
    }

    /// `evaluate_base` avec les tours de sortie par pièce déjà calculés.
    fn evaluate_base_with(&self, state: &State, side: u8, mut turns: [i32; MAX_PIECES]) -> i32 {
        if self.safe.is_some() {
            // Une Sentinelle ne compte que ses tours SÛRS (jamais moins que sa distance brute).
            for (i, p) in state.pieces().iter().enumerate() {
                if p.kind == Kind::Sentinel {
                    let safe = self.safe_turns(state, p, self.danger(state, i as u8));
                    turns[i] = turns[i].max(safe.min(turns[i] + self.w.safe_race));
                }
            }
        }
        let w = &self.w;
        let nobody_out = state.finished == 0;
        let other = if side == 1 { 2 } else { 1 };
        // Avec la course à la sortie, les points marqués (après une première sortie) ne valent
        // que par la course qu'ils ouvrent : l'écart de points brut n'est plus compté.
        let mut total = if w.exit_race == 0 {
            w.score
                * (state.scores[side as usize - 1] as i32 - state.scores[other as usize - 1] as i32)
        } else {
            0
        };
        let mut first = [i32::MAX; 2]; // pièce la plus rapide de chaque camp
        let mut sentinel = [i32::MAX; 2]; // Sentinelle la plus rapide de chaque camp
                                          // Tours de la pièce la plus rapide de chaque camp, par Force (indices 1 à 3).
        let mut fastest = [[i32::MAX; 4]; 2];
        // Tours des deux pièces les plus rapides de chaque camp, toutes Forces confondues.
        let mut two = [[i32::MAX; 2]; 2];
        for (i, p) in state.pieces().iter().enumerate() {
            let strength = p.kind.strength() as i32;
            let t = turns[i];
            let k = strength as usize - 1; // Explorateur 0, Avant-garde 1, Sentinelle 2
            let mut v = w.value[k] + (w.progress_horizon - t).max(0) * w.progress[k];
            if p.kind == Kind::Sentinel && nobody_out {
                v += (w.sentinel_horizon - t).max(0).pow(2) * w.sentinel_sq;
            }
            if p.kind == Kind::Sentinel {
                v += w.sentinel_alive;
                if w.sentinel_pressure != 0 {
                    // Force adverse au contact (elle s'ajoute à toute attaque contre elle),
                    // plafonnée à sa défense : c'est une valeur NÉGATIVE pour son camp.
                    let mut attack = 0;
                    for &nb in neighbors_of(p.sq) {
                        if let Some(j) = state.at(nb) {
                            let q = state.piece(j);
                            if q.owner != p.owner {
                                attack += q.kind.strength() as i32;
                            }
                        }
                    }
                    v -= w.sentinel_pressure * attack.min(Eval::defense(state, p));
                }
            }
            total += if p.owner == side { v } else { -v };
            let o = p.owner as usize - 1;
            first[o] = first[o].min(t);
            if p.kind == Kind::Sentinel {
                sentinel[o] = sentinel[o].min(t);
            }
            let f = &mut fastest[o][strength as usize];
            *f = (*f).min(t);
            if t < two[o][0] {
                two[o] = [t, two[o][0]];
            } else if t < two[o][1] {
                two[o][1] = t;
            }
        }
        if let Some(reach) = &self.reach {
            let pieces = state.pieces();
            for (i, p) in pieces.iter().enumerate() {
                let reaches = |q: &Piece| {
                    reach[q.kind.strength() as usize - 1][q.sq as usize] >> p.sq & 1 == 1
                };
                let strength = p.kind.strength() as i32;
                let v = if w.cover_cap == 0 {
                    let covered = pieces
                        .iter()
                        .enumerate()
                        .any(|(j, q)| j != i && q.owner == p.owner && reaches(q));
                    if covered {
                        w.cover * strength
                    } else {
                        0
                    }
                } else {
                    // Meilleure reprise si `p` est prise : le repreneur + nos autres voisines de la case.
                    let power = pieces
                        .iter()
                        .enumerate()
                        .filter(|&(j, q)| j != i && q.owner == p.owner && reaches(q))
                        .map(|(j, q)| {
                            let (hq, hs) =
                                (state.heights[q.sq as usize], state.heights[p.sq as usize]);
                            let height = (hq > hs) as i32 - (hq < hs) as i32;
                            let support: i32 = neighbors_of(p.sq)
                                .iter()
                                .filter_map(|&nb| {
                                    state.at(nb).map(|k| (k as usize, state.piece(k)))
                                })
                                .filter(|&(k, r)| k != j && k != i && r.owner == p.owner)
                                .map(|(_, r)| r.kind.strength() as i32)
                                .sum();
                            q.kind.strength() as i32 + height + support
                        })
                        .max()
                        .unwrap_or(0);
                    w.cover * strength * power.clamp(0, w.cover_cap) / w.cover_cap
                };
                total += if p.owner == side { v } else { -v };
            }
        }
        if w.tempo_race != 0 && nobody_out {
            let mine = Eval::plies(state, side, sentinel[side as usize - 1]);
            let theirs = Eval::plies(state, other, sentinel[other as usize - 1]);
            let margin = match (mine, theirs) {
                (i32::MAX, i32::MAX) => 0,
                (i32::MAX, _) => -w.tempo_cap,
                (_, i32::MAX) => w.tempo_cap,
                _ => (theirs - mine).clamp(-w.tempo_cap, w.tempo_cap),
            };
            total += w.tempo_race * margin;
        }
        if w.sentinel_threat != 0 {
            let mover = state.to_move;
            let threatened = state
                .pieces()
                .iter()
                .filter(|p| {
                    p.owner == mover && p.kind == Kind::Sentinel && self.capturable(state, **p)
                })
                .count() as i32;
            let v = w.sentinel_threat * threatened;
            total += if mover == side { -v } else { v };
        }
        if !nobody_out {
            if w.exit_race != 0 {
                total += self.exit_race(state, side, &fastest);
            }
        } else if w.start_race != 0 {
            total += self.start_race(state, side, &fastest, &two);
        } else {
            total += self.sentinel_race(state, side, &first, &sentinel);
        }
        total
    }

    /// Évaluation par le réseau (voir `Weights::nn`), du point de vue de `side`.
    fn evaluate_nn(&self, state: &State, side: u8) -> i32 {
        let (sparse, dense) = crate::nn::features(self, state);
        let logit = crate::nn::net().logit(&sparse, &dense).clamp(-30.0, 30.0);
        let mut v = (logit * self.w.nn as f32) as i32; // camp au trait
        if self.w.nn_race != 0 && state.finished > 0 && self.w.exit_race != 0 {
            let turns = self.piece_turns(state);
            let mut fastest = [[i32::MAX; 4]; 2];
            for (i, p) in state.pieces().iter().enumerate() {
                let f = &mut fastest[p.owner as usize - 1][p.kind.strength() as usize];
                *f = (*f).min(turns[i]);
            }
            v += self.exit_race(state, state.to_move, &fastest);
        }
        if side == state.to_move {
            v
        } else {
            -v
        }
    }

    /// Demi-coups avant que `owner` puisse jouer le `turns`-ième coup d'une pièce : le camp
    /// au trait joue le premier.
    #[inline]
    pub(crate) fn plies(state: &State, owner: u8, turns: i32) -> i32 {
        match turns {
            i32::MAX => i32::MAX,
            t if state.to_move == owner => 2 * t - 1,
            t => 2 * t,
        }
    }

    /// Chance (en millièmes) qu'un événement prévu dans `mine` demi-coups arrive avant un
    /// autre prévu dans `theirs` : 1/2 à marge nulle, linéaire jusqu'à la certitude à une
    /// marge de `±spread`, où `spread` grandit avec le délai (une course lointaine est
    /// incertaine : blocages, captures, détours).
    fn first_chance(&self, mine: i32, theirs: i32) -> i32 {
        match (mine, theirs) {
            (i32::MAX, _) => 0,
            (_, i32::MAX) => 1000,
            _ => {
                let spread = self.w.race_spread + mine.min(theirs) / self.w.race_spread_per.max(1);
                (500 + 500 * (theirs - mine) / spread.max(1)).clamp(0, 1000)
            }
        }
    }

    /// Course des Sentinelles, personne n'est sorti, du point de vue de `side`
    /// (`fastest[camp][Force]` : tours de la pièce la plus rapide de cette Force ;
    /// `two[camp]` : tours des deux plus rapides).
    ///
    /// Ma Sentinelle gagne si elle sort avant la Sentinelle adverse (R8.2) ET avant que
    /// l'adversaire ait fait DEUX sorties. Une seule sortie adverse plus faible ne la sauve
    /// pas : elle annule la victoire immédiate, mais ma Sentinelle sort ensuite et la partie
    /// s'arrête au score, 3 contre 1 ou 2 (R8.1). Une pièce bouge par tour : deux sorties
    /// coûtent la SOMME des tours des deux pièces. Chaque camp a sa chance ; la valeur est
    /// l'écart des deux.
    fn start_race(
        &self,
        state: &State,
        side: u8,
        fastest: &[[i32; 4]; 2],
        two: &[[i32; 2]; 2],
    ) -> i32 {
        let other = if side == 1 { 2 } else { 1 };
        let chance = |me: u8| {
            let them = if me == 1 { 2 } else { 1 };
            let sentinel = Eval::plies(state, me, fastest[me as usize - 1][3]);
            let [a, b] = two[them as usize - 1];
            let double = if b == i32::MAX { i32::MAX } else { a + b };
            let rescue = fastest[them as usize - 1][3].min(double);
            self.first_chance(sentinel, Eval::plies(state, them, rescue))
        };
        self.w.start_race * (chance(side) - chance(other)) / 1000
    }

    /// Course à la sortie décisive après une première sortie, de Force `a` (R8.1), du point
    /// de vue de `side` : la prochaine sortie termine la partie. Le camp en tête gagne avec
    /// n'importe quelle sortie (ou à la limite de coups) ; le camp mené doit sortir une pièce
    /// de Force > `a` avant lui (= `a` : nulle). C'est la règle « ne pas sortir sans plan » :
    /// une sortie qui ouvre une course perdue est notée comme telle, et la recherche l'évite.
    fn exit_race(&self, state: &State, side: u8, fastest: &[[i32; 4]; 2]) -> i32 {
        let any = |o: u8| {
            fastest[o as usize - 1][1..]
                .iter()
                .copied()
                .min()
                .unwrap_or(i32::MAX)
        };
        let race = self.w.exit_race;
        let leader: u8 = if state.scores[0] > state.scores[1] {
            1
        } else {
            2
        };
        let trailer = if leader == 1 { 2 } else { 1 };
        let lead = state.scores[leader as usize - 1] as usize;
        // La limite de coups départage au score : elle vaut une sortie du camp en tête.
        let remaining = state.move_limit as i32 - state.ply as i32 + 1;
        let lead_time = Eval::plies(state, leader, any(leader)).min(remaining);
        let t = &fastest[trailer as usize - 1];
        let stronger = t
            .get(lead + 1..)
            .and_then(|s| s.iter().copied().min())
            .unwrap_or(i32::MAX);
        let equal = t.get(lead).copied().unwrap_or(i32::MAX);
        let lose = self.first_chance(Eval::plies(state, trailer, stronger), lead_time);
        let draw =
            (1000 - lose) * self.first_chance(Eval::plies(state, trailer, equal), lead_time) / 1000;
        let v = race * (1000 - lose - draw - lose) / 1000;
        if side == leader {
            v
        } else {
            -v
        }
    }

    /// Course des Sentinelles (R8.2). Personne n'est sorti : une Sentinelle gagne si elle
    /// sort AVANT la Sentinelle adverse ET avant toute pièce adverse (une sortie adverse,
    /// même d'un Explorateur, annule la victoire immédiate). En demi-coups, le camp au trait
    /// joue le premier.
    fn sentinel_race(&self, state: &State, side: u8, first: &[i32; 2], sentinel: &[i32; 2]) -> i32 {
        let plies = |owner: u8, turns: i32| match turns {
            i32::MAX => i32::MAX,
            t if state.to_move == owner => 2 * t - 1,
            t => 2 * t,
        };
        let wins = |me: u8| {
            let them = if me == 1 { 2 } else { 1 };
            let mine = plies(me, sentinel[me as usize - 1]);
            mine < plies(them, first[them as usize - 1])
                && sentinel[me as usize - 1] <= self.w.sentinel_race_horizon
        };
        let other = if side == 1 { 2 } else { 1 };
        match (wins(side), wins(other)) {
            (true, false) => self.w.sentinel_race,
            (false, true) => -self.w.sentinel_race,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards;
    use traversal_bot::core::{apply_move, legal_moves, new_game, Ctx, Tuning};

    /// Hasard reproductible (splitmix64).
    fn next(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Avec les poids par défaut, même score que le SDK sur des positions de parties
    /// aléatoires (toutes les cartes, les deux points de vue, sorties comprises).
    #[test]
    fn same_as_sdk() {
        let mut seed = 7u64;
        let (mut checked, mut after_exit) = (0, 0);
        for heights in boards() {
            let sdk = Ctx::with_insight(&heights, 3, Tuning::default());
            let ours = Eval::new(&heights, Weights::sdk());
            let mut s = new_game(heights);
            while !s.over {
                for side in [1, 2] {
                    assert_eq!(
                        ours.evaluate(&s, side),
                        sdk.evaluate(&s, side),
                        "ply {} côté {side}",
                        s.ply
                    );
                    checked += 1;
                    after_exit += (s.finished > 0) as usize;
                }
                let moves = legal_moves(&s);
                s = apply_move(&s, &moves[(next(&mut seed) % moves.len() as u64) as usize]);
            }
            assert_eq!(ours.evaluate(&s, 1), sdk.evaluate(&s, 1));
        }
        assert!(checked > 10_000, "{checked} positions seulement");
        assert!(
            after_exit > 100,
            "{after_exit} positions après une sortie seulement"
        );
        eprintln!("{checked} positions comparées, dont {after_exit} après une sortie");
    }
}

#[cfg(test)]
mod race_tests {
    use super::*;
    use crate::search::{Config, Piolet};
    use traversal_bot::core::{apply_move, legal_moves, MoveKind, Piece};

    const FLAT: [u8; SQUARES] = [1; SQUARES];

    /// Seule la course compte (les deux phases) : les autres termes sont à zéro.
    fn race_only() -> Weights {
        Weights {
            start_race: 400,
            exit_race: 400,
            tempo_race: 0,
            value: [0; 3],
            progress: [0; 3],
            sentinel_sq: 0,
            ..Weights::default()
        }
    }

    fn piece(owner: u8, kind: Kind, sq: u8) -> Piece {
        Piece { owner, kind, sq }
    }

    /// Sur le plat, Rouge sort par la rangée 9, Bleu par la rangée 0 ; coût = rangées + 1.
    /// Rouge au trait. Explorateur rouge sur la rangée de sortie (1 tour), Sentinelle rouge
    /// rangée 5 (coût 5 : 2 tours), Sentinelle bleue rangée 4 (coût 5 : 2 tours).
    fn exit_without_plan() -> State {
        let pieces = [
            piece(1, Kind::Explorer, 92),
            piece(1, Kind::Sentinel, 57),
            piece(2, Kind::Sentinel, 42),
        ];
        State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200)
    }

    #[test]
    fn exiting_into_a_lost_race_is_bad() {
        let s = exit_without_plan();
        let e = Eval::new(&s.heights, race_only());
        // Sans sortie : la Sentinelle rouge (3 demi-coups) devance la bleue (4) : Rouge mène.
        let before = e.evaluate(&s, 1);
        assert!(before > 0, "avant : {before}");
        // Sortir l'Explorateur ouvre la course au score : Bleu (au trait) sort sa Sentinelle,
        // F3 > F1, avant la deuxième sortie rouge. Course perdue pour Rouge (d'un demi-coup :
        // l'ampleur dépend de `race_spread`).
        let exit = legal_moves(&s)
            .into_iter()
            .find(|m| m.kind == MoveKind::Exit)
            .expect("sortie");
        let after = apply_move(&s, &exit);
        assert!(
            e.evaluate(&after, 1) < 0,
            "après : {}",
            e.evaluate(&after, 1)
        );
        assert_eq!(e.evaluate(&after, 2), -e.evaluate(&after, 1));
    }

    #[test]
    fn search_does_not_exit_without_a_plan() {
        let s = exit_without_plan();
        let moves = legal_moves(&s);
        let never = || false;
        let pick = Piolet::new(Config {
            max_depth: 4,
            ..Config::default()
        })
        .choose(&s, &moves, never);
        assert_ne!(
            moves[pick.index].kind,
            MoveKind::Exit,
            "Piolet sort sans plan"
        );
    }

    #[test]
    fn a_weaker_enemy_exit_does_not_stop_the_sentinel() {
        // Personne n'est sorti. Sentinelle rouge à 2 tours ; Bleu n'a qu'un Explorateur à
        // 1 tour et une Sentinelle lointaine. Si Bleu sort l'Explorateur, la Sentinelle rouge
        // sort ensuite et gagne 3 à 1 : la course est rouge (le SDK la jugeait annulée).
        let pieces = [
            piece(1, Kind::Sentinel, 57),
            piece(2, Kind::Explorer, 3),
            piece(2, Kind::Sentinel, 82),
        ];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        let e = Eval::new(&s.heights, race_only());
        assert!(e.evaluate(&s, 1) >= 300, "{}", e.evaluate(&s, 1));
        // Même avec deux Explorateurs bleus à 1 tour : Bleu sort aux demi-coups 2 et 4, la
        // Sentinelle rouge au 3, entre les deux. Rouge gagne encore 3 à 1.
        let pieces = [
            piece(1, Kind::Sentinel, 57),
            piece(2, Kind::Explorer, 3),
            piece(2, Kind::Explorer, 4),
            piece(2, Kind::Sentinel, 82),
        ];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert!(e.evaluate(&s, 1) > 0, "{}", e.evaluate(&s, 1));
        // Sentinelle rouge rangée 3 (3 tours, demi-coup 5) : les deux sorties bleues passent
        // avant, la course n'est plus rouge.
        let pieces = [
            piece(1, Kind::Sentinel, 37),
            piece(2, Kind::Explorer, 3),
            piece(2, Kind::Explorer, 4),
            piece(2, Kind::Sentinel, 82),
        ];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert!(e.evaluate(&s, 1) <= 0, "{}", e.evaluate(&s, 1));
    }

    #[test]
    fn leader_wins_the_race_when_trailer_has_nothing_stronger() {
        // Rouge a sorti une Avant-garde (2 points) ; Bleu n'a plus que des Explorateurs : il
        // ne peut plus que perdre, même en sortant le premier.
        let pieces = [
            piece(1, Kind::Explorer, 12),
            piece(2, Kind::Explorer, 5),
            piece(2, Kind::Explorer, 6),
        ];
        let s = State::new(FLAT, &pieces, 2, 40, [2, 0], 1, 200);
        let e = Eval::new(&s.heights, race_only());
        assert_eq!(e.evaluate(&s, 1), 400);
    }
}

#[cfg(test)]
mod sentinel_tests {
    use super::*;
    use traversal_bot::core::Piece;

    const FLAT: [u8; SQUARES] = [1; SQUARES];

    fn piece(owner: u8, kind: Kind, sq: u8) -> Piece {
        Piece { owner, kind, sq }
    }

    /// Tous les termes à zéro, sauf ceux qu'on teste.
    fn nothing() -> Weights {
        Weights {
            value: [0; 3],
            progress: [0; 3],
            sentinel_sq: 0,
            sentinel_race: 0,
            exit_race: 0,
            ..Weights::sdk()
        }
    }

    /// Une Avant-garde seule (Force 2) ne prend une Sentinelle (défense 3) qu'avec la charge :
    /// les cases dangereuses sont exactement celles à coût de marche 4 d'elle.
    #[test]
    fn a_lone_vanguard_threatens_its_charge_ring() {
        let s = State::new(
            FLAT,
            &[piece(1, Kind::Sentinel, 11), piece(2, Kind::Vanguard, 55)],
            1,
            20,
            [0, 0],
            0,
            200,
        );
        let e = Eval::new(
            &FLAT,
            Weights {
                safe_race: 1,
                ..nothing()
            },
        );
        let ring = walk(&FLAT, 55, 4);
        let expected = (0..SQUARES)
            .filter(|&q| ring[q] == 4)
            .fold(0u128, |m, q| m | 1 << q);
        assert_eq!(e.danger(&s, 0), expected);
    }

    /// Sans adversaire, la distance sûre est la distance brute ; une Avant-garde bien placée sur la
    /// route l'allonge d'au moins un tour pour une position de départ.
    #[test]
    fn a_vanguard_on_the_route_costs_a_turn() {
        let e = Eval::new(
            &FLAT,
            Weights {
                safe_race: 1,
                ..nothing()
            },
        );
        let raw = exact_turns(&FLAT, 1, Kind::Sentinel);
        let alone = State::new(FLAT, &[piece(1, Kind::Sentinel, 44)], 1, 20, [0, 0], 0, 200);
        let base = e.safe_turns(&alone, &alone.pieces()[0], e.danger(&alone, 0));
        assert_eq!(base, raw[44] as i32);
        let longer = (0..SQUARES as u8).filter(|&v| v != 44).any(|v| {
            let s = State::new(
                FLAT,
                &[piece(1, Kind::Sentinel, 44), piece(2, Kind::Vanguard, v)],
                1,
                20,
                [0, 0],
                0,
                200,
            );
            e.safe_turns(&s, &s.pieces()[0], e.danger(&s, 0)) > base
        });
        assert!(longer);
    }

    #[test]
    fn a_defended_line_is_a_wall_for_the_enemy_sentinel() {
        // Rangée 3 rouge : E V V S E (colonnes 3 à 7). Les deux Avant-gardes ont 2 alliés
        // voisins : défense 4, une Sentinelle seule (attaque 3) ne passe pas.
        let pieces = [
            piece(1, Kind::Explorer, 33),
            piece(1, Kind::Vanguard, 34),
            piece(1, Kind::Vanguard, 35),
            piece(1, Kind::Sentinel, 36),
            piece(1, Kind::Explorer, 37),
            piece(2, Kind::Sentinel, 45),
        ];
        let s = State::new(FLAT, &pieces, 2, 20, [0, 0], 0, 200);
        let open = Eval::new(&FLAT, nothing()).blocked_costs(&s, 2)[45];
        let trap = Eval::new(
            &FLAT,
            Weights {
                wall_defense: 4,
                ..nothing()
            },
        )
        .blocked_costs(&s, 2)[45];
        // Sans piège : à travers l'Avant-garde (5 + 2 pour l'abattre). Avec : détour par la
        // colonne 2 (8).
        assert_eq!((open, trap), (7, 8));
    }

    #[test]
    fn a_threatened_sentinel_costs_the_side_to_move() {
        let w = Weights {
            sentinel_threat: 100,
            ..nothing()
        };
        let e = Eval::new(&FLAT, w);
        // Sentinelle bleue au contact de la rouge : attaque 3 contre défense 3, elle passe.
        let pieces = [piece(1, Kind::Sentinel, 55), piece(2, Kind::Sentinel, 56)];
        let red = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert!(e.capturable(&red, red.pieces()[0]));
        // Rouge au trait : sa Sentinelle est menacée, il doit y répondre.
        assert_eq!(e.evaluate(&red, 1), -100);
        assert_eq!(e.evaluate(&red, 2), 100);
        // Bleu au trait : la menace pèse sur la Sentinelle bleue, symétriquement.
        let blue = State::new(FLAT, &pieces, 2, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&blue, 2), -100);
        // Escortée par une Avant-garde au contact (65, voisins orthogonaux seulement) :
        // défense 4, la Sentinelle rouge ne craint plus rien ; la bleue, seule, reste à la
        // merci de la rouge (3 contre 3).
        let pieces = [
            piece(1, Kind::Sentinel, 55),
            piece(1, Kind::Vanguard, 65),
            piece(2, Kind::Sentinel, 56),
        ];
        let red = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert!(!e.capturable(&red, red.pieces()[0]));
        assert!(e.capturable(&red, red.pieces()[2]));
        assert_eq!(e.evaluate(&red, 1), 0);
        // Une pièce lointaine ne menace rien.
        let pieces = [piece(1, Kind::Sentinel, 5), piece(2, Kind::Sentinel, 95)];
        let red = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&red, 1), 0);
    }

    #[test]
    fn a_hunting_move_creates_a_threat_on_the_enemy_sentinel() {
        use traversal_bot::core::{legal_moves, MoveKind};
        let e = Eval::new(&FLAT, Weights::default());
        // Sentinelle rouge en 59 : la bleue (55) est hors de portée (3 de marche + 1 pour
        // attaquer > 3). Aller en 56 la met au contact : 3 contre 3, capturable au coup suivant.
        let pieces = [piece(1, Kind::Sentinel, 59), piece(2, Kind::Sentinel, 55)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        let moves = legal_moves(&s);
        let to = |sq: u8| {
            moves
                .iter()
                .find(|m| m.kind == MoveKind::Move && m.to == sq)
                .copied()
                .expect("coup")
        };
        assert!(!e.capturable(&s, s.pieces()[1]));
        assert!(e.hunts(&s, &to(56)));
        assert!(!e.hunts(&s, &to(69)));
    }

    #[test]
    fn tempo_race_counts_every_half_move() {
        let e = Eval::new(
            &FLAT,
            Weights {
                tempo_race: 10,
                ..nothing()
            },
        );
        // Sentinelle rouge rangée 5 (coût 5 : 2 tours), bleue rangée 4 (coût 5 : 2 tours).
        // Rouge au trait : 3 demi-coups contre 4, +1 pour Rouge.
        let pieces = [piece(1, Kind::Sentinel, 57), piece(2, Kind::Sentinel, 42)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 10);
        assert_eq!(e.evaluate(&s, 2), -10);
        // Sentinelle bleue rangée 7 (3 tours) : 3 contre 6 demi-coups, +3.
        let pieces = [piece(1, Kind::Sentinel, 57), piece(2, Kind::Sentinel, 72)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 30);
    }

    #[test]
    fn cover_values_pieces_an_ally_can_recapture() {
        let e = Eval::new(
            &FLAT,
            Weights {
                cover: 10,
                ..nothing()
            },
        );
        // Explorateur rouge en 44, Sentinelle rouge en 55 : diagonale, l'Explorateur couvre la
        // Sentinelle (+30) ; la Sentinelle couvre l'Explorateur (2 de marche ≤ 3 : +10).
        let pieces = [piece(1, Kind::Explorer, 44), piece(1, Kind::Sentinel, 55)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 40);
        // Trop loin l'une de l'autre (Sentinelle : 3 de Mouvement ; ici 6 de distance) : rien.
        let pieces = [piece(1, Kind::Sentinel, 0), piece(1, Kind::Sentinel, 6)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 0);
        // Symétrique : la même paire bleue compte contre Rouge.
        let pieces = [piece(2, Kind::Explorer, 44), piece(2, Kind::Sentinel, 55)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), -40);
    }

    #[test]
    fn cover_power_counts_the_recapture_strength() {
        let e = Eval::new(
            &FLAT,
            Weights {
                cover: 30,
                cover_cap: 3,
                ..nothing()
            },
        );
        // Sentinelle rouge en 55, couverte par un Explorateur seul (44, diagonale) : puissance
        // 1 -> 30 × 3 × 1/3 = 30. L'Explorateur, couvert par la Sentinelle (puissance 3, plafond) :
        // 30 × 1 × 3/3 = 30.
        let pieces = [piece(1, Kind::Explorer, 44), piece(1, Kind::Sentinel, 55)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 60);
        // + une Avant-garde voisine de la Sentinelle (56) : la reprise de l'Explorateur vaut
        // 1 + 2 = 3 (plafond) -> 90 pour la Sentinelle. L'Avant-garde est couverte par la
        // Sentinelle (3 + 0 : le seul autre voisin de 56 est la Sentinelle elle-même) -> 60 ;
        // l'Explorateur, par la Sentinelle (3) et l'Avant-garde (2 + 0) -> 30.
        let pieces = [
            piece(1, Kind::Explorer, 44),
            piece(1, Kind::Sentinel, 55),
            piece(1, Kind::Vanguard, 56),
        ];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 90 + 60 + 30);
    }

    #[test]
    fn weights_by_name() {
        let mut w = Weights::default();
        w.apply("value_e=20, progress_s=11,tempo_race=30").unwrap();
        assert_eq!(
            (w.value, w.progress[2], w.tempo_race),
            ([20, 28, 42], 11, 30)
        );
        assert_eq!(w.get("value_e"), Some(20));
        assert!(w.apply("inconnu=1").is_err());
        assert!(w.apply("value_e").is_err());
        for name in Weights::NAMES {
            assert!(Weights::default().get(name).is_some(), "{name}");
        }
    }

    #[test]
    fn nn_evaluation_is_antisymmetric() {
        let heights = crate::boards()[5];
        let e = Eval::new(
            &heights,
            Weights {
                nn: 270,
                ..Weights::default()
            },
        );
        let mut s = traversal_bot::core::new_game(heights);
        for _ in 0..12 {
            let v = e.evaluate(&s, s.to_move);
            assert_eq!(e.evaluate(&s, 3 - s.to_move), -v);
            assert!(v.abs() < 30 * 270 + 1);
            s = traversal_bot::core::apply_move(&s, &traversal_bot::core::legal_moves(&s)[0]);
        }
    }

    #[test]
    fn nn_correction_is_antisymmetric_and_bounded() {
        let heights = crate::boards()[5];
        let e = Eval::new(
            &heights,
            Weights {
                nn_resid: 270,
                ..Weights::default()
            },
        );
        let base = Eval::new(&heights, Weights::default());
        let mut s = traversal_bot::core::new_game(heights);
        for _ in 0..12 {
            let v = e.evaluate(&s, s.to_move);
            assert_eq!(e.evaluate(&s, 3 - s.to_move), -v);
            // La correction reste petite devant l'évaluation (réseau de l'étape 1 : logit < 5).
            assert!((v - base.evaluate(&s, s.to_move)).abs() <= 30 * 270);
            s = traversal_bot::core::apply_move(&s, &traversal_bot::core::legal_moves(&s)[0]);
        }
    }

    #[test]
    fn guard_squares_follow_the_enemy_sentinel_shortest_path() {
        // Sur le plat, la Sentinelle bleue en 45 descend tout droit : 35, 25, 15, 5. Elle est
        // préférée à l'Explorateur bleu, pourtant plus rapide.
        let pieces = [
            piece(1, Kind::Vanguard, 90),
            piece(2, Kind::Sentinel, 45),
            piece(2, Kind::Explorer, 18),
        ];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        let g = Eval::new(&FLAT, Weights::default()).guard_squares(&s, 2);
        let on: Vec<usize> = (0..SQUARES).filter(|&q| g[q]).collect();
        assert_eq!(on, vec![5, 15, 25, 35]);
    }

    #[test]
    fn pressure_on_the_enemy_sentinel_counts_up_to_its_defense() {
        let w = Weights {
            sentinel_pressure: 10,
            ..nothing()
        };
        let e = Eval::new(&FLAT, w);
        // Avant-garde rouge au contact de la Sentinelle bleue : 2 points de pression.
        let pieces = [piece(1, Kind::Vanguard, 54), piece(2, Kind::Sentinel, 55)];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 20);
        assert_eq!(e.evaluate(&s, 2), -20);
        // Deux Avant-gardes : 4, plafonné à la défense de la Sentinelle (3).
        let pieces = [
            piece(1, Kind::Vanguard, 54),
            piece(1, Kind::Vanguard, 56),
            piece(2, Kind::Sentinel, 55),
        ];
        let s = State::new(FLAT, &pieces, 1, 20, [0, 0], 0, 200);
        assert_eq!(e.evaluate(&s, 1), 30);
    }
}

#[cfg(test)]
mod distance_tests {
    use super::*;
    use crate::boards;

    const KINDS: [Kind; 3] = [Kind::Explorer, Kind::Vanguard, Kind::Sentinel];

    /// Sans coup spécial, on ne dépense pas plus de M par tour : le calcul exact ne descend
    /// jamais sous `ceil(coût / M)`. Et sur le plat, les deux coïncident (rangées + 1 de coût).
    #[test]
    fn exact_turns_is_a_tighter_bound() {
        for heights in boards() {
            for owner in [1, 2] {
                let cost = exit_costs(&heights, owner);
                for kind in [Kind::Vanguard, Kind::Sentinel] {
                    let exact = exact_turns(&heights, owner, kind);
                    for sq in 0..SQUARES {
                        assert!(
                            exact[sq] as i32 >= Eval::turns_for(cost[sq] as i32, kind),
                            "case {sq}"
                        );
                    }
                }
            }
        }
        let flat = [1; SQUARES];
        for kind in KINDS {
            let exact = exact_turns(&flat, 1, kind);
            let cost = exit_costs(&flat, 1);
            for sq in 0..SQUARES {
                assert_eq!(
                    exact[sq] as i32,
                    Eval::turns_for(cost[sq] as i32, kind),
                    "{kind:?} case {sq}"
                );
            }
        }
    }

    /// Vérité terrain : une pièce seule sur le plateau, parcours en largeur sur les vrais coups
    /// légaux du moteur (`legal_moves`) jusqu'à sa sortie. Doit coïncider avec la table.
    #[test]
    fn exact_turns_matches_the_engine() {
        use traversal_bot::core::{apply_move, legal_moves, MoveKind, Piece};
        let mut checked = 0;
        for heights in boards().into_iter().step_by(20) {
            for owner in [1u8, 2] {
                for kind in KINDS {
                    let table = exact_turns(&heights, owner, kind);
                    for sq in (0..SQUARES as u8).step_by(7) {
                        let mut seen = [false; SQUARES];
                        seen[sq as usize] = true;
                        let (mut frontier, mut turns) = (vec![sq], 0u8);
                        let truth = 'bfs: loop {
                            turns += 1;
                            let mut next = Vec::new();
                            for &from in &frontier {
                                let s = State::new(
                                    heights,
                                    &[Piece {
                                        owner,
                                        kind,
                                        sq: from,
                                    }],
                                    owner,
                                    0,
                                    [0, 0],
                                    0,
                                    200,
                                );
                                for m in legal_moves(&s) {
                                    if m.kind == MoveKind::Exit {
                                        break 'bfs turns;
                                    }
                                    let to = apply_move(&s, &m).pieces()[0].sq;
                                    if !seen[to as usize] {
                                        seen[to as usize] = true;
                                        next.push(to);
                                    }
                                }
                            }
                            if next.is_empty() {
                                break u8::MAX;
                            }
                            frontier = next;
                        };
                        assert_eq!(table[sq as usize], truth, "{kind:?} camp {owner} case {sq}");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 500);
    }

    /// Mesure (`cargo test --release -- --ignored --nocapture ecart`) : écart entre tours exacts
    /// et `ceil(coût / M)`, sur toutes les cases des 200 cartes.
    #[test]
    #[ignore]
    fn ecart_distance() {
        for kind in KINDS {
            let mut hist = std::collections::BTreeMap::<i32, usize>::new();
            let mut start = std::collections::BTreeMap::<i32, usize>::new();
            for heights in boards() {
                for owner in [1u8, 2] {
                    let exact = exact_turns(&heights, owner, kind);
                    let cost = exit_costs(&heights, owner);
                    for sq in 0..SQUARES {
                        *hist
                            .entry(exact[sq] as i32 - Eval::turns_for(cost[sq] as i32, kind))
                            .or_default() += 1;
                    }
                    // Cases de départ (colonnes 2 à 7 de la rangée de départ).
                    let row = if owner == 1 { 0 } else { SIZE - 1 };
                    for x in 2..8 {
                        let sq = row * SIZE + x;
                        *start
                            .entry(exact[sq] as i32 - Eval::turns_for(cost[sq] as i32, kind))
                            .or_default() += 1;
                    }
                }
            }
            eprintln!("{kind:?} : exact - ceil(coût/M) sur toutes les cases {hist:?} ; au départ {start:?}");
        }
    }
}
