# bot_traversal_primer — Piolet

Bot Rust / WebAssembly pour l'arène [Traversal](https://traversal-bot.app/).

- `src/search.rs` : négamax alpha-beta, approfondissement itératif, PVS, table de transposition Zobrist, recherche de calme, arrêt piloté par le fuel.
- `src/lib.rs` : interface WASM (`bot!(choose)`), dépense ≤ 50 % du fuel par coup.
- `HANDOFF.md` : contexte, règles, état d'avancement et prochaine étape (évaluation par poids).

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
# -> target/wasm32-unknown-unknown/release/piolet.wasm
```
