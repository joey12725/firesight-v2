# Contributing to FIRESight

Thanks for looking. Bug reports, questions about the model, and pull requests are all welcome. This page covers how to get the project running, where things live, and what a change needs before it can merge.

If you're new to the finance side, read the [primer](docs/primer.md) first. It explains the terms the code uses (withdrawal rate, funded, guardrails, Roth ladder and so on) in plain language.

## Ways to help

- **Report a bug.** Use the [bug report template](https://github.com/joey12725/firesight-v2/issues/new?template=bug_report.yml). An exported plan (click the household name at the top left and choose **Export**; change any personal numbers first if you like) makes almost any bug reproducible, because the same plan and seed give the same result on every machine.
- **Question a number.** If a result looks wrong, open a [model question](https://github.com/joey12725/firesight-v2/issues/new?template=model_question.yml). Say what you expected and why, ideally with a source. Several of the biggest fixes in this codebase started as "that can't be right".
- **Suggest a feature.** [Feature requests](https://github.com/joey12725/firesight-v2/issues/new?template=feature_request.yml) are most useful when they describe the decision you're trying to make, not only the control you want.
- **Send a pull request.** For anything larger than a small fix, open an issue first so we can agree on the approach.

## Setting up

You need [Rust](https://rustup.rs) (stable) with the `wasm32-unknown-unknown` target, and Node 22 or newer.

```bash
git clone https://github.com/joey12725/firesight-v2 && cd firesight-v2
rustup target add wasm32-unknown-unknown
cargo test --workspace --release        # the engine: about a minute on a laptop

cd web
npm install
npm run prepare-engine                  # wasm build + TypeScript types from the Rust schema + data files
npm run dev                             # http://localhost:5173
```

After you change anything under `crates/`, run `npm run prepare-engine` again so the browser picks up the new engine.

## Where things live

| You want to change | Look in |
|---|---|
| How a part of the household is modelled (tax, careers, housing, Social Security…) | `crates/firesight-core/src/<module>`, and the matching section of [docs/methodology.md](docs/methodology.md) |
| The plan format (fields a user can set) | `crates/firesight-core/src/model/`. TypeScript types are generated from it by `npm run schema`. |
| The monthly simulation loop | `crates/firesight-core/src/sim/` |
| What gets measured and summarised | `crates/firesight-core/src/stats/`, documented in [docs/engine-output.md](docs/engine-output.md) |
| Tax law and other parameters | `data/`, each folder with its sources. Scripts in `tools/` rebuild them. |
| The browser bridge and worker pool | `crates/firesight-wasm/`, `web/src/engine/` |
| A view or chart | `web/src/views/<view>/`, shared chart pieces in `web/src/ui/charts/` |
| The plan editor | `web/src/editor/` |
| The assistant | `web/src/assistant/` and [docs/assistant.md](docs/assistant.md) |

[docs/architecture.md](docs/architecture.md) is the design contract. Read sections 1 and 3 before changing the engine.

## Before you open a pull request

CI runs all of these, so it's faster to run them locally first:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release

cd web
npm run typecheck && npm run lint && npm run test
```

The engine has a few rules that its tests enforce:

- **Determinism.** The same plan and seed must give byte-identical results on any thread count, in the browser and natively. Use the engine's own RNG streams. Never iterate a `HashMap` where order matters, and never read the clock.
- **No allocation inside a simulated life.** `sim_no_alloc` checks this, because the engine's speed depends on it.
- **Money is conserved.** If you add a flow, it has to show up in the ledger identities checked by `sim_conservation`.
- **Golden results move on purpose.** If your change is meant to change results, update the digests with `FIRESIGHT_UPDATE_GOLDEN=1 cargo test -p firesight-core --test sim_golden` and say in the PR why the numbers moved, and by how much.
- **Performance.** For changes under `crates/`, run `tools/bench-check.sh`. It fails on a throughput regression over 25%.

For changes to the model, update [docs/methodology.md](docs/methodology.md) in the same PR. A relationship the docs don't describe is treated as a bug.

For UI changes, include a screenshot in the PR. `npm run smoke` and `npm run journeys` drive the real app in Playwright against a running dev server or preview (`BASE_URL=http://localhost:4173`).

## Writing style

The app and the docs use plain sentences and avoid jargon where a common word will do. Every chart states its finding in a sentence above it. Numbers are in today's dollars unless labelled otherwise. If you add a term a newcomer wouldn't know, add it to the [glossary](docs/primer.md#glossary).

## Regenerating the README images

The screenshots and animations are produced from the real app by scripts, so they stay honest and can be refreshed after a UI change:

```bash
cd web
npm run build && npx vite preview --port 4173 &

BASE_URL=http://localhost:4173 npm run docs-shots    # docs/images/*.png (needs Pillow for the size pass)
BASE_URL=http://localhost:4173 npm run docs-gifs     # docs/images/tour.gif, live-edit.gif (needs ffmpeg)
python3 ../tools/concept_figure.py                   # docs/images/one-line-vs-many-lives.svg
```

Pass a comma-separated list of names to either script to redo only some images, for example `npm run docs-gifs -- live-edit`.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
