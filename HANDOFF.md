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

## Course en temps (validée, par défaut : tempo_race 25)

Par demi-coup d'avance de notre meilleure Sentinelle sur la meilleure adverse (tours avec
blocages, trait inclus), borné à ± 6, tant que personne n'est sorti.

| Budget | Référence (faisceau 20) | tempo 25 | Améliorées / dégradées | En Bleu |
|---|---|---|---|---|
| 3 000 | 69,9 % | 70,4 % | 77 / 74 | 63,2 → 67,5 % |
| 3 600 | 69,1 % | 76,0 % | 91 / 63 | 60,8 → 69,8 % |
| 4 500 | 71,6 % | 76,2 % | 86 / 67 | — |

Cumul z ≈ +2,3. Arène wasm, 400 parties contre `montblanc` : **72,5 %** (Elo +168 ± 40), 0 faute.
tempo 10 : neutre ; tempo 25 + distance exacte : neutre (Bleu mieux, Rouge moins bien).

## Rôles des pièces (400 parties, examples/duel.rs --list)

- La Sentinelle de Piolet fait 238 des 411 captures : elle se bat au lieu de courir.
- Double sortie sans Sentinelle : 3 victoires de Piolet, 12 de Mont Blanc.
- Reprises (capture juste après une capture adverse) : Piolet 83, Mont Blanc 117. Un tiers des
  captures d'Explorateur sont des reprises (allonge, diagonales, bonds, + Force des alliés
  voisins de la cible). D'où le terme de couverture (`cover`).

## Couverture (négatif, laissée à 0)

Par-dessus la course en temps (76,0 % à 3 600 nœuds, 70,4 % à 3 000) :

| Variante | 3 600 nœuds | 3 000 nœuds |
|---|---|---|
| binaire, poids 5 | 71,6 % (56 / 73, z −1,5) | — |
| binaire, poids 15 | 64,9 % (52 / 97, z −3,7) | — |
| puissance (plafond 3), poids 2 | 71,8 % (58 / 73, z −1,3) | 70,1 % (69 / 73) |
| puissance (plafond 3), poids 5 | 69,8 % (58 / 86, z −2,3) | 69,5 % (76 / 81) |

Valoriser la couverture regroupe les pièces au lieu de les faire avancer : elle coûte des temps.
Les reprises sont l'affaire de la recherche (règle de combat exacte), pas de l'évaluation.

## Réglage automatique SPSA (examples/tune.rs) : pas de gain

383 itérations (arrêt par recyclage du conteneur), 16 parties d'auto-jeu par itération ;
moyenne des 100 dernières : value [18, 64, 78], progress [5, 9, 7], tempo_race 30,
sentinel_sq 5, start_race 38 (± 24, bruit), cover 1.

| Test | Actuel | Réglé |
|---|---|---|
| natif 3 000 / 3 600 / 4 500 (contre Mont Blanc) | 70,4 / 76,0 / 76,2 % | 70,8 / 71,4 / 73,4 % |
| wasm contre Mont Blanc (400) | 72,5 % | 73,0 % |
| wasm, réglé contre actuel (200) | — | 49,5 % |
| wasm contre sommet (200) | — | 84,5 % |

Non retenu. L'évaluation est plate autour des poids actuels : la limite est dans sa forme (une
somme de termes indépendants), pas dans ses coefficients. D'où Piolet-NN.

## Piolet-NN, étape 1 : le réseau prédit mieux que l'évaluation (go)

Données : `examples/dataset.rs`, auto-jeu de Piolet (3 000 nœuds, 4 premiers demi-coups au hasard),
~2 000 parties = 93 435 positions (arrêt par recyclage du conteneur). Entraînement `nn/train.py`
(numpy) sur les cartes 0-139, validation 140-159 (arrêt au meilleur point), test 160-199.

| Test (cartes 160-199) | log-loss | précision | début | milieu | fin |
|---|---|---|---|---|---|
| constante | 0,693 | — | | | |
| évaluation actuelle | 0,621 | 64,5 % | 0,678 | 0,604 | 0,562 |
| **réseau 625→32→32→1, cible mixte (résultat + recherche), L2 1e-3** | **0,603** | **65,3 %** | 0,661 | 0,587 | 0,539 |
| réseau, cible recherche | 0,610 | 64,9 % | | | |
| réseau sans les entrées binaires | 0,619 | 64,7 % | | | |
| réseau, cible résultat seul | 0,628 | 63,7 % | | | |
| recherche de Piolet | 0,584 | 66,5 % | 0,652 | 0,564 | 0,512 |

Gain du réseau sur l'évaluation : 0,018 de log-loss, IC 95 % par partie (bootstrap, 400 parties)
0,002 à 0,035. Premier essai (128 neurones, cible résultat, L2 1e-5) : surapprentissage dès la
2e époque (76 000 positions, ~1 600 parties seulement). La cible mixte (distillation de la
recherche) et la régularisation sont indispensables.

## Piolet-NN, étape 2 : le réseau seul joue bien moins bien (à diagnostiquer)

Inférence Rust (`src/nn.rs`, identique à numpy), réseau 625→32→32→1 de l'étape 1 à la place de
l'évaluation (`Weights::nn`, échelle 270 = pente de la régression logistique de l'évaluation) :

| Contre Mont Blanc | 3 600 nœuds | 3 000 nœuds |
|---|---|---|
| actuel | 76,0 % | 70,4 % |
| `nn=270` | 52,4 % (54 / 146, z −6,5) | 51,4 % (58 / 136, z −5,6) |
| `nn=270,nn_race=1` | 55,0 % (55 / 137, z −5,9) | 51,9 % (61 / 137, z −5,4) |

Le réseau prédit mieux l'issue (étape 1) mais départage mal les positions voisines que la
recherche compare, et n'a jamais vu de positions de feuilles (pièces en prise, échanges
inachevés). Natif : il est plus rapide que l'évaluation (16 parties : 9 s contre 16 s).
Pistes : mesurer l'accord avec le coup d'une recherche profonde ; hybride évaluation + k × réseau ;
entraîner aussi sur des positions de feuilles.

Infrastructure : le conteneur est recyclé environ toutes les 35 à 90 minutes ; tout calcul long
doit être reprenable (nn/generate.sh) ou tenir dans ce délai.

## Piolet-NN : apprendre à choisir le coup (positions profondes)

Génération : `examples/deep.rs` (auto-jeu à 20 000 nœuds ; positions critiques ou « difficiles » :
le coup d'une recherche de 3 000 nœuds est ≥ 30 points sous le meilleur ; 8 candidats), lots
reprenables par `nn/generate.sh`. ~14 parties/min, ~6 positions retenues par partie.

Deux pièges découverts en route :
- **Bornes prises pour des valeurs.** Les coups racine autres que le premier sont cherchés en
  fenêtre nulle : leur score est une borne (« pas mieux que le meilleur »), exactement égale au
  meilleur score pour 55 % des positions. Les étiqueter comme équivalents au meilleur = du bruit.
  `nn/rank.py` n'utilise que les comparaisons fiables : le meilleur coup (exact) contre un coup
  dont la borne est ≥ 20 points dessous (≈ 2 par position).
- **Égalités.** Le bon coup étant en première position, `argmax` favorisait l'évaluation (valeurs
  entières) : « 0,465 » est en fait 0,356 une fois les ex æquo comptés au prorata.

Sur ces comparaisons fiables, l'évaluation actuelle classe le meilleur coup au-dessus d'un coup
nettement moins bon dans seulement 63 à 68 % des cas (hasard : 50 %).

Réseau en correction (`rank.py --residual` : logit = évaluation / 270 + réseau, correction
initialisée à zéro), 5 113 positions (3 679 d'entraînement), test = cartes 160+ (911 positions,
1 619 paires) :

| Réseau | Test, toutes | Test, difficiles | Validation |
|---|---|---|---|
| évaluation actuelle | 0,677 | 0,621 | — |
| 32 neurones, L2 1e-3 | 0,710 | 0,644 | 0,749 |
| 32 neurones, L2 1e-2 | 0,720 | 0,648 | 0,754 |
| 16 neurones | 0,717 | 0,646 | 0,753 |
| **64 neurones, L2 1e-2** (choisi sur la validation) | **0,729** | **0,659** | **0,756** |
| 32 neurones, L2 3e-2 | 0,728 | 0,659 | 0,745 |

Gain du 64 neurones : +5,3 points, IC 95 % par position +1,4 à +9,2 (`nn/bootstrap.py`). Le
surapprentissage commence vers l'époque 10-15 ; arrêt au meilleur point de validation.
Premier essai (sans les paires fiables, perte softmax sur tous les candidats) : le réseau
dégradait la validation.

## Vitesse en wasm (gains gratuits, résultats de recherche identiques)

Mesure : journaux de l'arène (`--verbose`), fuel consommé / nœuds, 6 parties contre Mont Blanc. Le
fuel compte des instructions : la mesure ne dépend pas de la charge de la machine (contrairement au
temps natif).

| Version | Fuel par nœud | Nœuds par coup | Profondeur moyenne |
|---|---|---|---|
| avant | 334 k | 3 035 | 5,20 |
| file à seaux (Dial) dans `blocked_costs` | 211 k | 4 714 | 5,45 |
| + recherche de calme sans trier les coups calmes | **197 k** | **5 010** | **5,49** |

Les deux changements sont vérifiés par des tests d'identité (`same_as_sdk`, `tactical_matches_sdk_order`)
et par 16 parties identiques. Natif : évaluation ≈ 3,8 µs, coups triés du SDK ≈ 4,9 µs (dont 3,8 µs
de coups légaux) : les deux postes pèsent à peu près autant.

`PIOLET_SPEND` (part du fuel dépensée, %, 50 par défaut) : le fuel non dépensé est perdu ; le
dépassement maximal mesuré au-delà de la réserve est de 46 M (2 % du budget).

## Mesure : limites découvertes

- Le duel natif est déterministe : au plus 400 parties distinctes par budget de nœuds. Un échantillon
  de 400 parties donne un IC 95 % de ± 5,5 points sur l'écart apparié (`scripts/abpool.py`). Les petits
  effets (< 5 points) mesurés sur un seul échantillon ne sont pas tranchés.
- `duel --open N --oseed S` : N demi-coups d'ouverture au hasard, identiques pour les deux réglages
  comparés : des échantillons appariés indépendants, à cumuler.

## Ligue wasm : version accélérée, part du fuel

Arène officielle (fuel réel), même logique de recherche, seule la vitesse change :

| Contre | ancienne version (`tempo`) | accélérée, 50 % du fuel | accélérée, 80 % du fuel |
|---|---|---|---|
| Mont Blanc (400) | 72,5 % | **75,0 %** (Elo +191 ± 42) | 75,1 % |
| ancienne version (200) | — | **56,0 %** (+42 ± 50) | 52,8 % |
| sommet (200) | — | 80,2 % | 84,5 % |

0 faute ; fuel moyen 41 % (50 %) et 63 % (80 %). Version accélérée retenue (mêmes décisions, plus de
nœuds). 80 % du fuel : +50 % de nœuds sans gain mesurable : le calcul n'est plus le facteur
limitant. On garde 50 %.

## Piolet-NN sur les feuilles de la recherche

`examples/leaf.rs` (600 parties, 21 235 positions de feuilles de variante principale, étiquette =
recherche profonde de la racine). `nn/distill.py` : 14,0 % de la variance expliquée sur les cartes de
test (erreur 0,069 contre 0,080) ; correction prédite : écart-type 37 points (cible 76).
Distillation aux racines (80 000 positions) : 36,7 % expliqués, mais perte au jeu (76,0 → 67,6 %
à pleine échelle, 73,1 % à demi-échelle) : les racines ne ressemblent pas aux feuilles.

## Mesure robuste (1 600 parties appariées, 4 graines d'ouverture, 5 000 nœuds)

`duel --open 6 --oseed 1..4`, 400 parties par graine, cumul `scripts/abpool.py` :

| Contre la version actuelle (faisceau 20 : 76,2 %) | écart | IC 95 % | z |
|---|---|---|---|
| faisceau 24 | +0,6 | ± 2,7 | +0,45 |
| faisceau 28 | −0,4 | ± 2,7 | −0,30 |
| réseau des feuilles, nn_resid 270 | **−5,8** | ± 2,8 | **−4,1** |
| réseau des feuilles, nn_resid 135 | **−2,8** | ± 2,7 | **−2,0** |

- Faisceau : plateau entre 20 et 28 ; on garde 20.
- **Piolet-NN (correction d'évaluation par un petit réseau) : abandonné.** Toutes les variantes perdent
  (remplacement 52 %, paires sans ancrage 54 %, paires ancrées 66,5 %, distillation aux racines 67,6 /
  73,1 %, feuilles −5,8 / −2,8 points), avec une relation dose-effet nette : plus la correction pèse,
  plus on perd. Le réseau n'explique que 14 % de ce que la recherche profonde ajoute aux feuilles ; le
  reste de sa correction agit comme du bruit. Pistes si l'on y revient : entrées bien plus riches,
  apprentissage par renforcement en auto-jeu à grande échelle (machine dédiée).

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

## Distance sûre (`safe_race`) — protocole fixé AVANT les résultats (2026-10-24, 21:30 UTC)

Écrit et poussé avant d'avoir vu le moindre chiffre, pour qu'on ne puisse pas choisir la mesure après coup.

1. **Mesure principale** : `duel` à 5 000 nœuds, `--open 6`, graines 1 à 4, 400 parties chacune, appariées
   avec la référence `b20_s{1..4}` (même binaire, `safe_race=0`). Cumul par `scripts/abpool.py`.
2. **Deux variantes, toutes deux rapportées** : `safe_race=1` et `safe_race=2`. Aucune graine n'est écartée,
   aucune variante n'est ajoutée en cours de route sans être annoncée comme nouvelle hypothèse.
3. **Seuil** : deux variantes testées → retenue seulement si z ≥ 2,24 (Bonferroni, 5 % bilatéral).
4. **Confirmation sur données neuves** : la variante retenue rejoue sur les graines 5 à 8 (jamais vues).
   Elle doit y être positive avec z ≥ 1,64 seule. Sinon : non adoptée.
5. **Coût réel** : à nœuds égaux, le surcoût de l'évaluation est invisible. Dernière étape : duel wasm à
   fuel égal (budget de l'arène). Adoption seulement si elle reste positive là aussi.
6. **Mesure secondaire** (explicative, ne décide rien) : part des courses de Sentinelles gagnées.

### Résultats, étapes 1 à 3 (2026-10-25)

| | graine 1 | 2 | 3 | 4 | cumul 1 600 parties |
|---|---|---|---|---|---|
| `safe_race=1` | +6,1 | +6,3 | +8,9 | +5,4 | **+6,66 ± 2,66, z = 5,00** |
| `safe_race=2` | +7,4 | +3,5 | +11,5 | +6,6 | **+7,25 ± 2,70, z = 5,37** |

Les deux passent le seuil z ≥ 2,24. Retenue pour la confirmation : `safe_race=2`, plus grand écart cumulé
(la différence entre les deux n'est pas significative). Étape 4 : graines 5 à 8, référence rejouée sur ces graines.

### Résultats, étapes 4 et 5 — ADOPTÉ (2026-10-25)

Étape 4, graines neuves 5 à 8 (référence rejouée sur ces graines) :

| graine 5 | 6 | 7 | 8 | cumul 1 600 parties |
|---|---|---|---|---|
| +2,6 | +12,3 | +8,0 | +10,1 | **+8,25 ± 2,78, z = 5,94** (seuil 1,64) |

Étape 5, wasm à fuel égal (arène locale, 400 parties = 200 cartes × 2 couleurs). Coût d'abord réduit
(masques de cases attaquables : 13,5 -> 7,3 µs par évaluation, décisions identiques) :

| duel | résultat |
|---|---|
| `safe_race=2` contre l'ancienne version (dist 348443c5) | **71,5 %** (282 V / 8 N / 110 D), Elo +160 ± 40 |
| `safe_race=2` contre Mont Blanc | 79,9 % (319 / 1 / 80) |
| ancienne version contre Mont Blanc | 74,5 % (295 / 6 / 99) |

Aucune faute, fuel moyen 823 M par coup (41 % du budget). `safe_race: 2` passe dans `Weights::default()`.
Courses secondaires (explicatif) : graine 1, échanges de Sentinelles lancés 36 (réf. 28), subis 9 (réf. 14).

## Poids de l'échange (`trade_adv`, `hole`) — protocole fixé AVANT les résultats (2026-10-25)

Idée (utilisateur) : la valeur d'un échange de Sentinelle = l'avancée de la pièce qui reprend + l'espace
que la pièce prise laisse derrière. `examples/trades` (3 945 échanges) : l'échange vaut aujourd'hui −45
points pour le camp qui perd sa Sentinelle et reprend ; avec les variantes : T6 −38, T12 −30, H4 −39, H8 −33.

- `trade_adv` : points par rangée au-delà de la ligne médiane, par Explorateur et Avant-garde.
- `hole` : points par case de la moitié adverse qu'aucune pièce adverse ne peut attaquer (moins l'inverse).

1. Référence : version adoptée (`safe_race=2`), listes `safeab/r2_s{1..4}` (mêmes décisions que le binaire actuel).
2. Quatre variantes, toutes rapportées : T6 (`trade_adv=6`), T12 (`trade_adv=12`), H4 (`hole=4`), H8 (`hole=8`).
   5 000 nœuds, `--open 6`, graines 1 à 4, 400 parties chacune, appariées. Cumul `scripts/abpool.py`.
3. Seuil : quatre variantes -> z ≥ 2,50 (Bonferroni, 5 % bilatéral).
4. Variante retenue (plus grand écart cumulé parmi celles qui passent) : confirmation graines 5 à 8 contre
   `confirm/r2_s{5..8}`, z ≥ 1,64. Une combinaison T + H ne serait testée qu'ensuite, comme nouvelle hypothèse.
5. Wasm à fuel égal : face-à-face contre la version adoptée, et contre Mont Blanc.
6. Secondaire (explicatif) : nombre d'échanges de Sentinelles et score des parties avec échange.

### Résultats — NON ADOPTÉ (2026-10-25)

| variante | graine 1 | 2 | 3 | 4 | cumul 1 600 parties |
|---|---|---|---|---|---|
| T6 (`trade_adv=6`) | +0,9 | +4,5 | +0,6 | −1,9 | +1,03 ± 2,17, z = 0,95 |
| T12 (`trade_adv=12`) | −0,6 | +2,3 | −1,5 | −5,5 | −1,34 ± 2,30, z = −1,17 |
| H4 (`hole=4`) | −2,9 | +1,9 | −2,0 | −1,5 | −1,12 ± 2,45, z = −0,92 |
| H8 (`hole=8`) | −4,0 | −3,3 | −3,6 | −6,0 | **−4,22 ± 2,55, z = −3,31** |

Aucune ne passe le seuil (2,50) : pas de confirmation ni de wasm. H8 dégrade nettement. Échanges (graines 1-2,
parties avec échange) : réf. 43 / 51, T6 47 / 48, H8 38 / 53 (score 58 % sur la graine 2, contre 65 %).
Les termes restent dans le code, coupés (0).

## Arène réelle (export de 29 parties, 2026-10-26)

Le site génère ses terrains (modes woodlands, valleys, desert, mountains, highlands + graine) : ce ne sont
pas les 200 cartes du SDK. Notre moteur rejoue les 29 parties sans désaccord de règles ; Piolet retrouve 87 %
de ses coups (43 % sans `safe_race`) : c'est bien la version adoptée. Bilan : bots maison 9 V / 1 N, Piolet v1
3-2, Tainted_Soul 1-1, chilling bot 4-6, Tar_Soul 0-2. Défaites : courses de Sentinelles adverses lancées dès
le premier coup (17 à 37 demi-coups). Diagnostics (`examples/arena_games`) : Sentinelle adverse à 2 tours
jugée « sans chemin sûr » près de notre camp ; coup de course adverse élagué par le faisceau.

Adversaires de test ajoutés à `duel` : `--terrains data/arena/terrains.txt` (15 terrains réels),
`--opp rush` (Mont Blanc fonceur naïf : Piolet 97-98 %, trop faible, écarté), `--opp-w` (un autre Piolet).
Rival « Fonceur » retenu : `--opp-w "progress_s=40,sentinel_sq=20,tempo_race=50"` (Piolet 47,5 % contre lui).

### `race` et `safe_walls` — protocole fixé AVANT les résultats

- R : `--race-moves 1` (coups de Sentinelle vers une case de sortie en un tour exemptés du faisceau)
- W : `--w safe_walls=1` (les pièces bloquent la marche des attaquants dans la distance sûre)
- RW : les deux.

1. Deux bancs, 5 000 nœuds, `--open 6`, graines 1 à 4, 400 parties appariées chacune :
   (a) Mont Blanc, cartes SDK, référence `safeab/r2_s*` ; (b) rival Fonceur (5 000 nœuds), terrains réels,
   référence rejouée (`railab/ref_s*`).
2. Mesure principale : cumul des deux bancs (3 200 parties). Seuil : trois variantes -> z ≥ 2,39.
   Condition supplémentaire : aucun banc significativement négatif (z < −1,64 sur 1 600 parties).
3. Retenue : plus grand écart parmi celles qui passent ; confirmation graines 5 à 8 (deux bancs), z ≥ 1,64 ;
   puis wasm à fuel égal (face-à-face contre la version adoptée et contre Mont Blanc).

### Remplacé avant tout résultat : protocole en deux étages (2026-10-26)

Le banc `race` / `safe_walls` ci-dessus a été arrêté avant la fin de son premier lot (aucun résultat lu) : le
second export (40 parties) montre deux défauts plus nets — faux gains par élagage de la défense « en ombre »
(`ov6vflcj`), adversaire fonceur sous-estimé (`Lm4k1Hpq`) — et la v2 perd 4-6 contre la v1 dans l'arène.

Variantes : SB (`--sentinel-beam 1`), SS (`--w safe_self=1`), SBSS (les deux), R (`--race-moves 1`).
Bancs (5 000 nœuds, `--open 6`, 400 parties appariées par graine) :
- MB : Mont Blanc, cartes SDK (références `safeab/r2_s1..4`, `confirm/r2_s5..8`) ;
- V1 : Piolet v1 (`--opp-w safe_race=0`, 5 000 nœuds), terrains réels `terrains2.txt` ;
- FO : Fonceur (`--opp-w progress_s=40,sentinel_sq=20,tempo_race=50`), terrains réels (étage 2 seulement).

1. Étage 1 (tri) : graines 1-2, bancs MB et V1. Passe à l'étage 2 si cumul (1 600 parties) z ≥ 1,0.
2. Étage 2 (décision, données neuves) : graines 5-8, bancs MB, V1, FO (4 800 parties). Seuil Bonferroni selon
   le nombre k de variantes qualifiées (k = 1 : 1,96 ; 2 : 2,24 ; 3 : 2,39 ; 4 : 2,50) ; aucun banc < −1,64.
3. Wasm à fuel égal : face-à-face contre la version adoptée, contre la v1 et contre Mont Blanc.

#### Étage 1 (graines 1-2, 800 parties par banc)

| variante | MB | V1 (terrains réels) | cumul 1 600 | étage 2 ? |
|---|---|---|---|---|
| SB (`sentinel_beam`) | +5,6 ± 3,5 | +9,3 ± 4,1 | **+7,4 ± 2,7, z = 5,58** | oui |
| R (`race`) | +3,3 ± 1,7 | +1,6 ± 2,2 | +2,4 ± 1,4, z = 3,59 | oui |
| SS (`safe_self`) | −12,1 | −25,1 | −18,6, z = −12,3 | non |
| SBSS | −3,2 | −16,4 | −9,8, z = −6,8 | non |

`safe_self` est très mauvais : la distance sûre de l'adversaire compte autant que la nôtre. k = 2 -> seuil 2,24.

#### Étage 2 (graines neuves 5-8, 1 600 parties par banc) — seuil 2,24

| variante | MB | V1 (terrains réels) | Fonceur (terrains réels) | cumul 4 800 |
|---|---|---|---|---|
| SB (`sentinel_beam`) | +4,7 ± 2,4 | +6,1 ± 2,9 | +8,8 ± 3,3 | **+6,52 ± 1,67, z = 7,82** |
| R (`race`) | +3,4 ± 1,2 | +2,3 ± 1,3 | +2,5 ± 1,4 | +2,75 ± 0,74, z = 7,39 |

Les deux passent, sur chaque banc. Retenue : SB (plus grand écart). Étape 3 : wasm à fuel égal
(`sb.wasm` contre la version adoptée, contre la v1 et contre Mont Blanc).

#### Étape 3, wasm à fuel égal — ADOPTÉ (2026-10-26)

| duel (400 parties) | résultat |
|---|---|
| `sentinel_beam` contre la version adoptée (v2) | **58,1 %** (226 V / 13 N / 161 D), Elo +57 ± 35 |
| `sentinel_beam` contre la v1 | 75,0 % (v2 contre v1 : 71,5 %) |
| `sentinel_beam` contre Mont Blanc | **89,5 %** (v2 : 79,9 %) |

Aucune faute, fuel moyen 828 à 933 M par coup. `sentinel_beam: true` dans `Config::default()`.

## Arbitrage à la limite de coups (`adjudge`) — protocole fixé AVANT les résultats (2026-10-26)

Troisième export (Piolet v3, 40 parties) : 2 défaites à la limite de 200 demi-coups (0-0, arbitrage par la pièce
la plus avancée, `rules::adjudicate`), dont une contre Mont Blanc après 30 coups d'allers-retours. Dans les duels
locaux, 1 à 3,5 % des parties vont à la limite. Terme : `adjudge` points au vainqueur de l'arbitrage, montant
linéairement sur les `adjudge_window` (40) derniers demi-coups.

Variantes : A3 (`adjudge=300`), A10 (`adjudge=1000`). Référence : version adoptée (`sentinel_beam`), listes
`stage/SB_{mb,v1,fo}_s5..8`. Mesure : graines 5-8, trois bancs (4 800 parties), seuil z ≥ 2,24 (deux variantes),
aucun banc < −1,64 ; confirmation graines 9-12 (trois bancs, z ≥ 1,64) ; wasm à fuel égal. Secondaire : défaites
à la limite. Effet attendu faible (≤ 1,5 point) : un échec au seuil n'est pas une preuve d'inutilité.

## Répétitions (`Config::repeat`) — protocole fixé AVANT les résultats (2026-10-26)

Troisième export : allers-retours (une pièce revient sur sa case en ≤ 4 demi-coups) 223 dans les 16 défaites
contre 88 dans les 24 victoires ; `iKpyx58M` (Mont Blanc) : ~90 allers-retours de Sentinelles qui s'« ombrent »
(a8/c8 contre a10/c10) pendant 180 demi-coups, perdu à l'arbitrage alors que Piolet s'évaluait à +395.
Correction : une position déjà vue dans la partie (à notre trait ou après notre coup) ou sur le chemin de la
recherche vaut 0. Variante unique RP (`--repeat 1`), référence `stage/SB_*`, graines 5-8, trois bancs, seuil
z ≥ 1,96, aucun banc < −1,64 ; confirmation graines 9-12 z ≥ 1,64 ; wasm. Lancé après le test `adjudge`.

### Résultats `adjudge` — NON ADOPTÉ (2026-10-26)

| variante | MB | V1 | FO | cumul 4 800 |
|---|---|---|---|---|
| A3 (300) | −0,19 | −0,16 | −0,12 | −0,16 ± 0,18, z = −1,71 |
| A10 (1 000) | −0,25 | −0,38 (z = −2,06) | −0,38 | −0,33 ± 0,23, z = −2,95 |

Parties à la limite : 104 (réf.) -> 103 (A3) / 91 (A10) ; victoires à la limite 57 -> 53 / 42 ; défaites 37 -> 39 / 39.
Le terme ne change presque rien (peu de parties concernées) et ce qu'il change est légèrement négatif : il pousse
à avancer une pièce exposée en fin de partie. Coupé (0).

### Résultats `repeat` — NON ADOPTÉ (2026-10-26)

MB −1,22 ± 1,08 (z = −2,25) ; V1 −0,19 ; FO +0,09 ; cumul 4 800 : −0,44 ± 0,86 (z = −1,02). Effet pervers : parties
à la limite 104 -> 170, défaites à la limite 37 -> 124 (une répétition vaut 0 aussi pour le camp en retard, qui la
recherche ; et le camp en avance, privé de son aller-retour, ne trouve pas mieux). Les allers-retours sont un
symptôme : il manque un plan pour percer le blocage. Coupé.

## Percer un blocage (`unblock`) — protocole fixé AVANT les résultats (2026-10-26)

Sentinelle « bloquée » : distance (pièces et danger compris) > distance sur le relief seul. Terme : points par pas
de rapprochement (rayon 8, Manhattan) de nos deux pièces non-Sentinelles les plus proches. Sur `iKpyx58M`, il ne
change pas le coup : c'est une poursuite mutuelle (chaque Sentinelle prend l'autre si elle est à 2 cases sans
soutien ; quitter la danse pour amener un soutien perd la Sentinelle), que la recherche à profondeur 4 ne résout pas.
Mesure générale quand même : U15 (`unblock=15`), U40 (`unblock=40`), référence `stage/SB_*`, graines 5-8, trois
bancs, seuil z ≥ 2,24, aucun banc < −1,64 ; confirmation graines 9-12 ; wasm.

### Résultats `unblock` — NON ADOPTÉ, très nuisible (2026-10-26)

U15 : cumul −25,8 ± 1,8 (MB −19,9 ; V1 −24,7 ; FO −32,9). U40 : −71,6 (5,8 % de score !). La condition « bloquée »
(distance avec pièces > distance sur le relief) est vraie presque tout le temps (la moindre pièce sur la route
suffit) : le terme, jusqu'à 2 × 8 × poids points par Sentinelle, attire toutes les pièces autour de nos Sentinelles
au lieu de jouer. Un terme de « plan » statique ne remplace pas la recherche. Coupé.
