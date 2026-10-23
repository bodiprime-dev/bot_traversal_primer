# Piolet — bot pour l'arène Traversal : point de passage

Document de reprise (à donner à Claude Code). Il résume ce qu'on a établi et où on en est.

## Objectif

Construire **notre propre bot** pour la « guerre de bots » de https://traversal-bot.app/ et l'inscrire dans l'arène. Décision prise avec l'utilisateur (Quentin, dév back-end, parle technique, veut le code expliqué) :

- ne pas se contenter de benchmarker le bot de départ ;
- **définir des poids pour les différentes actions**, c'est-à-dire écrire notre propre fonction d'évaluation, paramétrée par des poids ajustables, plutôt que réutiliser celle de Mont Blanc.

## Le site et le format de soumission

- Un bot = un module **WebAssembly (.wasm)**, envoyé sur https://traversal-bot.app/arena/new (compte requis). Il affronte les autres bots en continu et grimpe au classement (rangs : https://traversal-bot.app/ranks).
- SDK Rust : https://github.com/Sylv-Lej/traversal-rust-sdk, tag courant **v0.2.0**. Crates : `traversal-bot` (SDK), `traversal-core` (moteur : règles, coups légaux, `apply_move`, évaluation, format des tours). Autres langages possibles (C, Zig, Go) via l'interface brute : exports `memory`, `traversal_buffer() -> i32` (tampon de 16 Ko), `traversal_move(len: i32) -> i32` ; imports permis seulement `traversal.fuel_left() -> i64` et `traversal.log(ptr, len)`.
- Contraintes du bac à sable : **2 milliards de fuel par coup** (≈ instructions wasm, remis à neuf à chaque coup ; à zéro la partie est perdue), 64 Mo de mémoire, module ≤ 1 Mo, aucun accès système (ni horloge ni réseau), hasard = `turn.seed`. Un plantage ou un indice de coup invalide fait perdre. La mémoire du bot **persiste d'un coup à l'autre** dans une partie.
- API du bot : `fn choose(turn: &Turn) -> usize` + `bot!(choose);`. `turn.state` (position), `turn.moves` (coups légaux), `turn.seed`, `turn.fuel`.
- Outil local officiel (Docker) : `docker build -f arena/Dockerfile -t traversal-arena .` puis `docker run --rm -v "$PWD":/w traversal-arena duel /w/mon_bot.wasm sommet --games 20` (adversaires : `galet`, `sentier`, `crete`, `sommet`, `montblanc`, ou un .wasm). Aussi `check` pour valider un .wasm. `--verbose` affiche les `log`.

## Règles du jeu (résumé)

- Plateau 10×10 en relief (hauteurs 0 à 6, deux cases voisines diffèrent d'au plus 2). 2 joueurs, 6 pièces chacun. Rouge part de la rangée 1 et sort par la 10, Bleu l'inverse ; Rouge commence.
- Pièces (Mouvement + Force = 6) : **Explorateur** (M5/F1 ; pas en diagonale ou bond de 2 cases en ligne droite sans coût de hauteur, jamais par-dessus une pièce), **Avant-garde** (M4/F2 ; +1 en attaque si le plus court chemin vers la cible coûte exactement 4), **Sentinelle** (M3/F3 ; si c'est la toute première pièce sortie, son camp gagne immédiatement). 2 de chaque par camp, placées S V E E V S sur les colonnes c à h.
- Déplacement : une pièce par tour, case par case en ligne droite. Même hauteur : coût 1 ; changement de hauteur : écart + 1. On ne traverse aucune pièce.
- Combat : on termine son déplacement sur la case ennemie. Attaque = Force + 1 si on part plus haut (−1 si plus bas) + Force de chaque autre pièce alliée voisine de la cible. Défense = Force + 1 par allié voisin du défenseur. Attaque ≥ défense : la pièce attaquée sort sans marquer et on prend sa case, sinon l'attaque est interdite.
- Sortie : depuis la dernière rangée avec ≥ 1 point de Mouvement restant ; marque la Force. Dès que **2 pièces** (tous camps) sont sorties, meilleur score gagne (égalité = nulle). Limite de 200 demi-coups.
- Règles complètes : https://traversal-bot.app/rules (le code de `core/src/rules.rs`, `movegen.rs` fait foi).

## Ce qui existe (dans le sandbox de Claude, à récupérer via les fichiers ci-dessous)

Crate **`piolet`** (cdylib + rlib), dépendance `traversal-bot` en git tag v0.2.0. Le code complet est en annexe.

- `src/search.rs` : **notre moteur de recherche** : négamax alpha-beta, approfondissement itératif, PVS, table de transposition Zobrist (2^17 entrées, conservée d'un coup à l'autre, réinitialisée si nouvelle partie), recherche de calme (captures et sorties, 4 demi-coups), faisceau sur les coups calmes (`beam`, 10 par défaut), arrêt piloté par le fuel (vérifié tous les 64 nœuds). Règle sur la dernière itération : on garde la dernière itération complète, ou le meilleur coup d'une itération interrompue s'il bat le coup principal précédent.
- `src/lib.rs` : interface WASM, dépense au plus la moitié du fuel du coup (`SPEND_NUM/SPEND_DEN`), log de diagnostic par coup.
- `examples/duel.rs` : duel **natif** Piolet vs Mont Blanc (budget en nœuds au lieu de fuel). Le fichier n'est pas reproduit en annexe : à réécrire si besoin.
- **L'évaluation est encore celle du SDK** (`Ctx::with_insight(.., 3, Tuning::default())`). C'est la prochaine pièce à remplacer.

Constat de calibrage (natif, petit échantillon) : Mont Blanc (profondeur 4, beam 7, insight 3) dépense en moyenne ≈ 3300 nœuds par coup ; Piolet avec un budget de 6000 nœuds atteint une profondeur moyenne de ≈ 5,3. Premier duel de 10 parties : +4 =0 −6 (40 %), trop peu de parties pour conclure ; les variantes de beam/budget n'ont pas été terminées. **Rien n'est prouvé sur la force relative.** À re-mesurer avec plus de parties une fois l'évaluation propre écrite.

## Mesures en vrai wasm (arène officielle, v0.2.0, 2 000 M de fuel par coup)

Outil : `scripts/arena.sh` (arène du SDK compilée en natif, sans Docker). Le `.wasm` (66 Ko) passe `check`.
Évaluation encore celle du SDK ; dépense plafonnée à 50 % du fuel.

| Adversaire | Parties | V / N / D | Score | Elo (±) | Fuel moyen / coup |
|---|---|---|---|---|---|
| galet | 40 | 40 / 0 / 0 | 100 % | — | 748 M (37 %) |
| sentier | 40 | 39 / 0 / 1 | 97,5 % | +636 ± 563 | 745 M (37 %) |
| crete | 40 | 28 / 2 / 10 | 72,5 % | +168 ± 152 | 878 M (44 %) |
| sommet | 40 | 27 / 0 / 13 | 67,5 % | +127 ± 137 | 868 M (43 %) |
| montblanc | 40 | 19 / 3 / 18 | 51,2 % | +9 ± 112 | 910 M (46 %) |

Contre Mont Blanc (20 parties, 462 coups journalisés) :
- **≈ 280 k fuel par nœud** compté (les feuilles de la recherche de calme coupées par `stand >= beta`
  ne sont pas comptées : le coût réel par évaluation est plus bas). ≈ 3 600 nœuds par coup.
- Profondeur moyenne 5,1 (de 1 à 16 en fin de partie).
- Pire coup : il restait 954 M, soit un dépassement d'au plus 46 M au-delà de la réserve
  (contrôle tous les 64 nœuds ≈ 18 M). Aucune faute sur 200 parties.

Conséquences :
- Le coût par nœud est dominé par l'évaluation et le tri des coups du SDK. Notre évaluation
  devra rester bon marché, sinon on perd en profondeur ce qu'on gagne en finesse.
- La marge est large : passer `SPEND` de 1/2 à ~4/5 donnerait ~60 % de nœuds en plus sans
  risque (dépassement observé < 50 M). À mesurer.

## Mesure : qui sort le premier, et ce que ça donne (duel natif, 400 parties)

`cargo run --release --example duel -- --games 400` : Piolet (3 600 nœuds, évaluation = SDK)
contre Mont Blanc natif. Le harnais reproduit l'arène wasm (40 parties : 52,5 % natif contre
51,2 % wasm). Total : 191 V / 17 N / 192 D (49,9 %).

| Première sortie | Parties | Résultat pour Piolet | Score |
|---|---|---|---|
| Sentinelle de Piolet (victoire immédiate) | 150 | 150 / 0 / 0 | 100 % |
| Sentinelle de Mont Blanc | 144 | 0 / 0 / 144 | 0 % |
| Piolet, Explorateur | 22 | 3 / 1 / 18 | 16 % |
| Piolet, Avant-garde | 28 | 5 / 9 / 14 | 34 % |
| Mont Blanc, Explorateur | 20 | 14 / 3 / 3 | 78 % |
| Mont Blanc, Avant-garde | 20 | 11 / 3 / 6 | 63 % |
| aucune sortie (limite de coups) | 16 | 8 / 1 / 7 | 53 % |

- **74 % des parties se jouent sur la course des Sentinelles** (294 sur 400).
- Sortir le premier hors Sentinelle est perdant pour les deux bots ; Piolet le fait plus souvent
  (50 contre 40) et y marque 26 %. Après une sortie de Piolet hors Sentinelle, la sortie suivante
  est 28 fois sur 50 une Sentinelle de Mont Blanc (F3 > F1 ou F2 : Mont Blanc gagne).
- Corrélation, pas encore causalité : une partie de ces sorties a pu être le meilleur essai dans
  une position déjà perdue. Le test A/B du terme `exit_race` tranchera.

## Étape exit_race : « ne pas sortir sans plan » (validée)

Après une première sortie de Force `a`, la prochaine sortie termine la partie : le camp en
tête gagne avec n'importe quelle sortie (ou à la limite de coups), le camp mené doit sortir
une pièce de Force > `a` avant lui. `Eval::exit_race` note cette course (marge en demi-coups,
incertitude `race_spread + d / race_spread_per`) à la place de l'écart de points brut.

A/B apparié natif contre Mont Blanc (400 parties chacun, mêmes cartes et couleurs) :

| Budget | Référence (SDK) | exit_race | Améliorées / dégradées | z |
|---|---|---|---|---|
| 3 000 nœuds | 47,5 % | 53,5 % | 67 / 43 | +2,29 |
| 3 600 nœuds | 49,9 % | 55,8 % | 75 / 53 | +1,94 |
| 4 500 nœuds | 47,6 % | 50,4 % | 76 / 63 | +1,10 |
| cumul | | | 218 / 159 | ≈ +3,0 |

Arène wasm officielle, 400 parties contre `montblanc` : **47,8 % → 53,1 %** (−16 → +22 Elo,
± 34), 0 faute, fuel moyen 43 %. Piolet sort le premier hors Sentinelle ~16 fois sur 400 au
lieu de ~50 ; quand Mont Blanc le fait, Piolet y marque ~75 %.

`start_race` (course des Sentinelles réécrite : une sortie adverse plus faible ne sauve pas de
la Sentinelle, deux sorties coûtent la somme des tours) est implémentée et testée mais
**désactivée** : elle dégrade la course des Sentinelles (A+B : 50,6 % ; A seule : 50,5 %).
Piste : le modèle est juste sur le papier, mais la valeur graduée remplace un ±150 à horizon 5
qui marchait ; à reprendre avec un réglage (poids, horizon) plutôt que jetée.

À surveiller : à 4 500 nœuds, la course des Sentinelles se dégrade aussi avec exit_race seule
(133 / 165 contre 142 / 152).

## Essai : poids d'attaque et de piège des Sentinelles adverses (négatif)

`wall_defense`, `sentinel_pressure`, `sentinel_alive` (voir `eval.rs`), chacun seul, A/B
apparié contre la version `exit_race` (55,8 %, 400 parties, 3 600 nœuds) :

| Variante | Score | Améliorées / dégradées | z | Sentinelles P / MB |
|---|---|---|---|---|
| référence | 55,8 % | — | — | 145 / 146 |
| `--wall 4` | 53,6 % | 15 / 24 | −1,44 | 142 / 154 |
| `--pressure 10` | 55,1 % | 56 / 58 | −0,19 | 142 / 141 |
| `--pressure 25` | 54,2 % | 66 / 72 | −0,51 | 149 / 141 |
| `--alive 40` | 54,9 % | 49 / 52 | −0,30 | 137 / 146 |
| `--alive 100` | 50,9 % | 51 / 70 | −1,73 | 129 / 163 |

Aucun gain : les trois restent à 0. Lecture : `alive` fort rend Piolet frileux avec sa propre
Sentinelle (course perdue plus souvent) ; `wall` ne touche que 39 parties (les lignes défendues
sont rares) et pénalise aussi notre propre Sentinelle face aux leurs.

Piste non testée : le problème est peut-être dans la recherche, pas dans l'évaluation. Le tri
des coups calmes (SDK, `ordered`) ne regarde que l'avancée vers la sortie ; avec le faisceau
(10 coups calmes par nœud, 16 à la racine), un coup qui barre le chemin de la Sentinelle
adverse sans avancer peut n'être jamais examiné. Mont Blanc ajoute ces parades à la racine
(`guard_squares`, `guard_extra`) ; pas Piolet.

## Essai : coups de parade contre la Sentinelle adverse (neutre)

`Config::guard_root` / `guard_tree` (cases d'un plus court chemin de la Sentinelle adverse,
`Eval::guard_squares`), A/B apparié contre la version `exit_race` (55,8 %) :

| Variante | Score | Améliorées / dégradées | z |
|---|---|---|---|
| `--guard-root 2` | 54,2 % | 41 / 47 | −0,64 |
| `--guard-root 4` | 54,1 % | 51 / 59 | −0,76 |
| `--guard-tree 1` | 54,5 % | 80 / 87 | −0,54 |
| `--guard-tree 2` | 56,5 % | 92 / 89 | +0,22 |
| `--guard-root 2 --guard-tree 1` | 56,6 % | 96 / 95 | +0,07 |

Rien de significatif : laissés à 0.

### Pourquoi la course des Sentinelles résiste (diagnostic sur la référence)

- C'est un **sprint** : la Sentinelle gagnante sort au demi-coup 20 en médiane (10 coups par
  camp). Peu de temps pour bloquer, encercler ou piéger.
- **Le trait compte** : Rouge (qui joue en premier) gagne 164 courses, Bleu 127.
- Sur les 110 cartes où les deux parties (couleurs inversées) finissent en course de
  Sentinelles, 56 sont gagnées par la même **couleur** des deux côtés (la carte et le trait
  décident, pas le bot), 54 par le même **bot** : 27 pour Piolet, 27 pour Mont Blanc.

Conséquence : le gain possible est dans la moitié « décidée par le bot », et ce qui y compte est
de courir au plus juste. Piste : l'évaluation estime les tours comme `ceil(coût / Mouvement)`,
ce qui est faux sur le relief (une marche de coût 3 ne se coupe pas en deux tours). Une
distance exacte en tours, par type de pièce (budget de Mouvement par tour, bonds et diagonales
de l'Explorateur), se précalcule une fois par coup : coût nul par nœud.

## Essai : distance exacte en tours (neutre)

`exact_turns` (`eval.rs`) : tours minimaux pour sortir, par camp et type de pièce, sur le relief
seul ; vérifiée contre un parcours en largeur sur les coups légaux du moteur
(`exact_turns_matches_the_engine`). `ceil(coût / M)` sous-estime 22 % des Sentinelles au
départ (d'1 ou 2 tours), 13 % des cases pour la Sentinelle, 7 % pour l'Avant-garde.

A/B apparié contre la version `exit_race` :

| Variante | 3 000 nœuds | 3 600 nœuds | 4 500 nœuds |
|---|---|---|---|
| référence | 53,5 % | 55,8 % | 50,4 % |
| `--exact 1` | 51,4 % (60 / 68) | 53,5 % (61 / 73) | 50,6 % (66 / 63) |
| `--exact 1 --start 400` | 54,5 % (90 / 85) | 51,2 % (79 / 96) | — |

Rien de significatif : `exact_turns` reste à `false`. La distance est juste mais ne change pas
l'issue : près de la sortie, la recherche voit les vrais coups ; de loin, l'erreur ne semble pas
décider des parties.

Constat à creuser : le score de référence ne croît pas avec le budget (3 000 : 53,5 % ; 3 600 :
55,8 % ; 4 500 : 50,4 %). Si plus de nœuds n'aident pas, accélérer l'évaluation n'aidera pas
non plus ; à vérifier avec des budgets bien plus grands (×2, ×4) avant d'y investir.

## Mesure : le budget de nœuds compte-t-il ? (peu)

Version `exit_race`, 400 parties contre Mont Blanc par budget (A/B apparié contre 3 600) :

| Nœuds par coup | 3 000 | 3 600 | 4 500 | 7 200 (×2) | 14 400 (×4) |
|---|---|---|---|---|---|
| Score | 53,5 % | 55,8 % | 50,4 % | 52,9 % (80 / 90) | 60,8 % (101 / 80, z = +1,56) |

L'écart-type d'un score sur 400 parties est ≈ 2,5 points : jusqu'à ×2, la courbe est plate ;
×4 donne peut-être +5 points, à la limite du significatif. Une accélération réaliste de
l'évaluation (×1,5 à ×2) ne rapporterait donc presque rien : ce n'est pas le levier.

Conséquence méthodologique : un écart de ±3 points entre deux runs de 400 parties est du bruit.
Pour départager deux réglages proches, cumuler plusieurs budgets (comme pour exit_race) ou
plus de cartes.

## Stratégie : ce que disent les parties

- **Les 200 cartes sont symétriques** (`examples/openings.rs`) : mêmes distances pour les deux
  camps ; la seule asymétrie est le trait (Rouge gagne 164 courses de Sentinelles, Bleu 127).
- **La Sentinelle pourrait sortir en 4 à 6 tours** (90 % des cartes), mais la gagnante arrive
  avec 6 tours de retard en médiane : aucun des deux bots ne mène de vrai sprint.
- **Le sprint pur échoue** (`--sprint 1` : 2 V / 38 D sur 40) : 53 Sentinelles capturées
  (Piolet normal : 8). Seule, une Sentinelle (défense 3) est prise par une Sentinelle ou une
  Avant-garde soutenue ; chaque allié au contact lui donne +1 en défense. Le jeu est une course
  en convoi, pas un sprint.

## Faisceau de 20 coups calmes (validé, par défaut)

| Coups calmes par nœud | 6 | 10 | 14 | 20 | illimité |
|---|---|---|---|---|---|
| 3 600 nœuds | 42,8 % | 55,8 % | 60,1 % | **69,1 %** (z +4,0) | 60,9 % |
| 3 000 nœuds | 41,1 % | 53,5 % | 67,0 % | **69,9 %** (z +4,5) | 60,4 % |

**Arène wasm officielle, 400 parties contre `montblanc` : 70,6 %** (279 V / 7 N / 114 D,
Elo +152 ± 39 ; version précédente 53,1 %), 0 faute, fuel moyen 42 % du budget.

Le tri des coups calmes (SDK) ne regarde que l'avancée vers la sortie : à 10, les coups
d'escorte et de défense n'étaient jamais examinés. Au-delà de 20, la recherche se disperse.

## Menace réelle et chasse (neutres avec le faisceau de 20)

- `sentinel_threat` (pénalité du camp au trait par Sentinelle capturable, `Eval::capturable`
  sur les coups du moteur) : 50 → 67,5 % (72 / 80), 150 → 65,9 % (70 / 83), contre 69,1 %.
- `Config::hunt` (coups qui rendent une Sentinelle adverse capturable, protégés de l'élagage) :
  2 → 69,5 % (57 / 55), 4 → 68,8 % (63 / 64).

Laissés à 0 : avec un faisceau assez large, la recherche voit déjà ces coups ; le budget étant
compté en nœuds en natif, ce n'est pas une question de coût.

## Analyse : Bleu (examples/blue.rs, examples/captures.rs)

Piolet (faisceau 20) en Bleu contre Mont Blanc, 200 cartes : 121 gagnées, 56 perdues sur la
Sentinelle rouge. En Rouge, Piolet fait 77 % ; en Bleu, 61 à 63 %.

`tempo::sprint_turns` (tours exacts pour sortir si rien d'autre ne bouge, coups du moteur) et
`tempo::tempo_balance` (bilan d'un coup en tours). Gain net = perdus par eux + gagnés par nous − 1.

| Parties perdues (56) | |
|---|---|
| tours où un coup « 2 contre 1 » (gain net ≥ 1) existait / joué | 80 sur 833 / 11 |
| tours où Bleu joue un coup à −1 alors que le meilleur valait 0 | 536 (64 %) |
| Bleu en tête sans jouer sa Sentinelle / avancée sûre disponible | 109 / 54 |

Une capture termine le déplacement : un bloqueur capturable coûte quand même un tour à la
Sentinelle adverse (échange 1 contre 1).

**Captures de Sentinelle rouge refusées : 159 cas, presque tous justifiés.** 81 : la capture
était suivie d'une reprise (souvent notre Sentinelle prend la leur et se fait reprendre) ; 47 :
le coup joué gagnait (sortie, souvent de notre Sentinelle) ; 25 : la recherche juge le coup joué
au moins aussi bon ; 6 seulement où elle préfère la capture. Les refus ont coûté 6 parties, dont
une (carte 182) où deux Sentinelles se font face 20 demi-coups durant, chacune prenable et
reprenable : un face-à-face que l'évaluation juge nul et que Rouge finit par gagner.

Conclusion : le problème de Bleu n'est pas tactique (captures) mais le temps perdu à chaque
coup, que l'évaluation ne mesure pas.

## Prochaine étape : évaluation par poids d'actions

Remplacer `Ctx::evaluate` par notre fonction, avec une structure de poids réglable, par exemple :

```rust
pub struct Weights {
    pub progress: i32,        // avancée vers la sortie (coût de chemin relief, par pièce, pondéré par le Mouvement)
    pub strength_value: i32,  // valeur d'une pièce selon sa Force
    pub score_diff: i32,      // points déjà marqués (sorties)
    pub sentinel_race: i32,   // Sentinelle la plus proche de sortir en premier
    pub enemy_exit_threat: i32, // pièce adverse proche de la sortie (annule la victoire Sentinelle)
    pub support_neighbors: i32, // alliés voisins (défense + force d'attaque)
    pub height_advantage: i32,  // être plus haut que les voisins ennemis
    pub charge_ready: i32,      // Avant-garde à coût exact 4 d'une cible attaquable
    pub explorer_leap: i32,     // bonds/diagonales utiles (contourne le relief)
    pub blocked_path: i32,      // chemin barré par une pièce ennemie / alliée
    pub attack_gain: i32,       // valeur d'une capture réalisable (attaque ≥ défense)
    pub exposure: i32,          // pièce attaquable par l'adversaire au prochain coup
}
```

Pistes :
1. Réutiliser les briques de `traversal_bot::core` (`exit_costs`, `neighbors_of`, `step_cost`, `legal_moves`) pour les distances, le relief et les combats ; ne pas tout réimplémenter.
2. Poids de départ raisonnés à la main, puis **réglage par auto-jeu** (SPSA ou recherche locale, duels natifs avec couleurs inversées et plusieurs dizaines de cartes de `boards.json`, 200 cartes fournies dans le dépôt du SDK).
3. Mesurer à fuel réel avec l'outil Docker officiel contre `sommet` et `montblanc` avant de soumettre ; surveiller le fuel moyen par coup.
4. Garder une marge de fuel (aujourd'hui on dépense ≤ 50 %).

## Contraintes d'environnement rencontrées

- ~~Dans le sandbox de Claude, `rustup target add wasm32-unknown-unknown` échoue~~ (résolu : la cible s'installe désormais, voir les mesures ci-dessus) : le proxy refuse `static.rust-lang.org` (403). **Le `.wasm` n'a donc jamais été compilé ni testé en wasm.** Le code compile en natif (`cargo check`, `cargo build --release --example duel`). À faire dans un environnement normal : `rustup target add wasm32-unknown-unknown` puis `cargo build --release --target wasm32-unknown-unknown` → `target/wasm32-unknown-unknown/release/piolet.wasm`.
- Points à vérifier en vrai wasm : le `thread_local!` du bot, la taille du `.wasm` (≤ 1 Mo), l'absence d'import interdit (le SDK n'importe que `traversal.fuel_left` et `traversal.log`), la consommation réelle de fuel par nœud (non mesurée ; le commentaire du bot de départ évoque ~20 M de fuel pour 64 nœuds sur Mont Blanc).

## Annexe : code actuel

### `Cargo.toml`

```toml
[package]
name = "piolet"
version = "0.1.0"
edition = "2021"
rust-version = "1.80"
publish = false
description = "Bot Traversal : alpha-beta (PVS, table de transposition, recherche de calme) borné par le fuel."

[lib]
# cdylib : le .wasm de l'arène. rlib : pour l'exemple de duel natif.
crate-type = ["cdylib", "rlib"]

[dependencies]
traversal-bot = { git = "https://github.com/Sylv-Lej/traversal-rust-sdk", tag = "v0.2.0" }

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
```

### `src/lib.rs`

```rust
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
```

### `src/search.rs`

```rust
//! Moteur de recherche de Piolet : alpha-beta négamax avec
//!  - approfondissement itératif (la dernière itération complète fait foi),
//!  - PVS (fenêtre nulle sur les coups non principaux, re-recherche si nécessaire),
//!  - table de transposition (Zobrist) conservée d'un coup à l'autre pendant la partie,
//!  - recherche de calme (captures et sorties) à l'horizon,
//!  - arrêt piloté par l'appelant (`stop`), car il n'y a pas d'horloge en WebAssembly.
//!
//! L'évaluation est celle du moteur du SDK (`Ctx::evaluate`, niveau de compréhension 3).

use std::cmp::Reverse;

use traversal_bot::core::bot::WIN;
use traversal_bot::core::{apply_move, Ctx, Kind, Move, MoveKind, State, Tuning};

const INF: i32 = 1_000_000_000;
/// Profondeur max de la recherche de calme (captures / sorties enchaînées).
const QPLIES: u8 = 4;
/// Taille de la table de transposition (puissance de 2).
const TT_BITS: usize = 17;

/// Réglages de la recherche.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Profondeur maximale (demi-coups) de l'approfondissement itératif.
    pub max_depth: u8,
    /// Coups « calmes » explorés par nœud, en plus des captures et sorties (toujours toutes
    /// explorées). `usize::MAX` : pas d'élagage.
    pub beam: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config { max_depth: 16, beam: 10 }
    }
}

struct Timeout;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flag {
    Exact,
    Lower,
    Upper,
}

#[derive(Clone, Copy)]
struct Entry {
    key: u64,
    score: i32,
    mv: Option<Move>,
    depth: i8,
    flag: Flag,
}

const EMPTY: Entry = Entry { key: 0, score: 0, mv: None, depth: -1, flag: Flag::Exact };

/// Résultat d'un choix : le coup, et de quoi alimenter un message de diagnostic.
#[derive(Clone, Copy, Debug)]
pub struct Pick {
    /// Indice dans la liste de coups fournie.
    pub index: usize,
    /// Profondeur de la dernière itération complète.
    pub depth: u8,
    pub score: i32,
    pub nodes: u64,
}

/// Le bot : porte la table de transposition d'un coup à l'autre.
pub struct Piolet {
    cfg: Config,
    tt: Vec<Entry>,
    zob: Vec<u64>,
    /// Garde-fous : on repart de zéro si la position n'est pas la suite de la précédente.
    last_ply: Option<u16>,
    last_heights: Option<[u8; 100]>,
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn kind_idx(k: Kind) -> usize {
    match k {
        Kind::Explorer => 0,
        Kind::Vanguard => 1,
        Kind::Sentinel => 2,
    }
}

impl Piolet {
    pub fn new(cfg: Config) -> Piolet {
        let mut seed = 0xC0FF_EE15_600D_F00D_u64;
        // 6 types (camp × pièce) × 100 cases, + 4 clés annexes (trait, scores, sortis).
        let zob = (0..6 * 100 + 4).map(|_| splitmix(&mut seed)).collect();
        Piolet { cfg, tt: vec![EMPTY; 1 << TT_BITS], zob, last_ply: None, last_heights: None }
    }

    fn hash(&self, s: &State) -> u64 {
        let mut h = 0u64;
        for p in s.pieces() {
            h ^= self.zob[((p.owner as usize - 1) * 3 + kind_idx(p.kind)) * 100 + p.sq as usize];
        }
        if s.to_move == 2 {
            h ^= self.zob[600];
        }
        h ^= self.zob[601].wrapping_mul(1 + s.scores[0] as u64);
        h ^= self.zob[602].wrapping_mul(1 + s.scores[1] as u64);
        h ^= self.zob[603].wrapping_mul(1 + s.finished as u64);
        h
    }

    /// Choisit un coup parmi `moves` (la liste légale fournie par l'arène).
    /// `stop` est interrogé tous les 64 nœuds ; à `true`, la recherche s'arrête et on garde
    /// la dernière itération complète.
    pub fn choose(&mut self, state: &State, moves: &[Move], stop: fn() -> bool) -> Pick {
        // Nouvelle partie (ou autre relief) : la table n'a plus de sens.
        let fresh = match (self.last_ply, self.last_heights) {
            (Some(p), Some(h)) => state.ply <= p || h != state.heights,
            _ => true,
        };
        if fresh {
            self.tt.iter_mut().for_each(|e| *e = EMPTY);
        }
        self.last_ply = Some(state.ply);
        self.last_heights = Some(state.heights);

        let none = Pick { index: 0, depth: 0, score: 0, nodes: 0 };
        if moves.len() <= 1 {
            return none;
        }
        let ctx = Ctx::with_insight(&state.heights, 3, Tuning::default());
        let mut s = Search { owner: self, ctx: &ctx, stop, nodes: 0 };
        s.root(state, moves)
    }
}

struct Search<'a> {
    owner: &'a mut Piolet,
    ctx: &'a Ctx,
    stop: fn() -> bool,
    nodes: u64,
}

impl Search<'_> {
    fn tick(&mut self) -> Result<(), Timeout> {
        self.nodes += 1;
        if self.nodes % 64 == 0 && (self.stop)() {
            return Err(Timeout);
        }
        Ok(())
    }

    fn same(a: &Move, b: &Move) -> bool {
        a.piece == b.piece && a.kind == b.kind && a.to == b.to
    }

    fn root(&mut self, state: &State, moves: &[Move]) -> Pick {
        // Coups racine, les plus prometteurs d'abord (tri du moteur), avec leur indice.
        let mut root: Vec<(Move, usize)> = self
            .ctx
            .ordered(state)
            .into_iter()
            .map(|m| {
                let i = moves.iter().position(|x| Self::same(x, &m)).unwrap_or(0);
                (m, i)
            })
            .collect();
        let mut best = Pick { index: root[0].1, depth: 0, score: 0, nodes: 0 };
        let mut best_mv = root[0].0;
        let beam = self.owner.cfg.beam;

        'deepen: for depth in 1..=self.owner.cfg.max_depth {
            let mut scored: Vec<(i32, (Move, usize))> = Vec::with_capacity(root.len());
            let mut alpha = -INF;
            for (n, &(m, idx)) in root.iter().enumerate() {
                // Au-delà du faisceau, un coup calme n'est essayé qu'aux premières itérations.
                if n >= beam && depth > 2 && m.kind == MoveKind::Move && n >= beam + 6 {
                    scored.push((-INF, (m, idx)));
                    continue;
                }
                let child = apply_move(state, &m);
                let r = if n == 0 {
                    self.negamax(&child, depth - 1, -INF, INF)
                } else {
                    // Fenêtre nulle d'abord ; re-recherche pleine si le coup améliore alpha.
                    match self.negamax(&child, depth - 1, -alpha - 1, -alpha) {
                        Ok(v) if -v > alpha => self.negamax(&child, depth - 1, -INF, -alpha),
                        other => other,
                    }
                };
                match r {
                    Ok(v) => {
                        let score = -v;
                        scored.push((score, (m, idx)));
                        alpha = alpha.max(score);
                    }
                    Err(Timeout) => {
                        // Itération interrompue : on garde son meilleur coup seulement s'il a
                        // été entièrement évalué ET dépasse le coup principal de l'itération
                        // précédente (qui est toujours essayé en premier).
                        if let Some(&(sc, (mv, i))) = scored.iter().max_by_key(|(sc, _)| *sc) {
                            if n > 0 && mv != best_mv && sc > best.score {
                                best = Pick { index: i, depth: depth - 1, score: sc, nodes: self.nodes };
                            }
                        }
                        break 'deepen;
                    }
                }
            }
            // Itération complète : tri par score (stable : l'ordre précédent départage).
            scored.sort_by_key(|(sc, _)| Reverse(*sc));
            let (score, (mv, idx)) = scored[0];
            best = Pick { index: idx, depth, score, nodes: self.nodes };
            best_mv = mv;
            root = scored.into_iter().map(|(_, x)| x).collect();
            // Victoire forcée trouvée : inutile d'aller plus loin.
            if score >= WIN - 1000 {
                break;
            }
        }
        best.nodes = self.nodes;
        best
    }

    fn probe(&self, key: u64) -> Option<Entry> {
        let e = self.owner.tt[(key as usize) & ((1 << TT_BITS) - 1)];
        (e.key == key && e.depth >= 0).then_some(e)
    }

    fn store(&mut self, key: u64, depth: u8, score: i32, flag: Flag, mv: Option<Move>) {
        let slot = &mut self.owner.tt[(key as usize) & ((1 << TT_BITS) - 1)];
        // Remplacement : on garde l'entrée plus profonde de la même position.
        if slot.key == key && slot.depth > depth as i8 && flag != Flag::Exact {
            return;
        }
        *slot = Entry { key, score, mv, depth: depth as i8, flag };
    }

    fn negamax(&mut self, state: &State, depth: u8, mut alpha: i32, mut beta: i32) -> Result<i32, Timeout> {
        if state.over {
            return Ok(self.ctx.evaluate(state, state.to_move));
        }
        if depth == 0 {
            return self.quiesce(state, alpha, beta, QPLIES);
        }
        self.tick()?;

        // Près de la limite de coups, le score dépend du nombre de demi-coups restants :
        // la table (indexée sans lui) n'est pas fiable.
        let use_tt = state.ply as u32 + depth as u32 + QPLIES as u32 + 2 < state.move_limit as u32;
        let key = self.owner.hash(state);
        let (alpha0, mut tt_move) = (alpha, None);
        if use_tt {
            if let Some(e) = self.probe(key) {
                tt_move = e.mv;
                if e.depth >= depth as i8 {
                    match e.flag {
                        Flag::Exact => return Ok(e.score),
                        Flag::Lower => alpha = alpha.max(e.score),
                        Flag::Upper => beta = beta.min(e.score),
                    }
                    if alpha >= beta {
                        return Ok(e.score);
                    }
                }
            }
        }

        let mut moves = self.ctx.ordered(state);
        if let Some(t) = tt_move {
            if let Some(i) = moves.iter().position(|m| *m == t) {
                moves[..=i].rotate_right(1);
            }
        }

        let (mut best, mut best_move) = (-INF, None);
        let mut quiet = 0usize;
        for (n, m) in moves.iter().enumerate() {
            if m.kind == MoveKind::Move {
                // Coups calmes : seuls les `beam` premiers (hors coup de la table) sont explorés.
                if n > 0 && quiet >= self.owner.cfg.beam {
                    continue;
                }
                quiet += 1;
            }
            let child = apply_move(state, m);
            let score = if n == 0 {
                -self.negamax(&child, depth - 1, -beta, -alpha)?
            } else {
                let v = -self.negamax(&child, depth - 1, -alpha - 1, -alpha)?;
                if v > alpha && v < beta {
                    -self.negamax(&child, depth - 1, -beta, -alpha)?
                } else {
                    v
                }
            };
            if score > best {
                best = score;
                best_move = Some(*m);
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }

        if use_tt {
            let flag = if best <= alpha0 {
                Flag::Upper
            } else if best >= beta {
                Flag::Lower
            } else {
                Flag::Exact
            };
            self.store(key, depth, best, flag, best_move);
        }
        Ok(best)
    }

    /// Au bout de l'horizon : on poursuit les captures et les sorties jusqu'au calme. Le camp
    /// au trait peut « ne rien faire » : son évaluation sur place sert de plancher.
    fn quiesce(&mut self, state: &State, mut alpha: i32, beta: i32, plies: u8) -> Result<i32, Timeout> {
        let stand = self.ctx.evaluate(state, state.to_move);
        if state.over || plies == 0 || stand >= beta {
            return Ok(stand);
        }
        self.tick()?;
        alpha = alpha.max(stand);
        let mut best = stand;
        for m in self.ctx.ordered(state) {
            if !matches!(m.kind, MoveKind::Exit | MoveKind::Attack) {
                break; // triés : sorties et captures d'abord
            }
            let v = -self.quiesce(&apply_move(state, &m), -beta, -alpha, plies - 1)?;
            best = best.max(v);
            alpha = alpha.max(v);
            if alpha >= beta {
                break;
            }
        }
        Ok(best)
    }
}
```
