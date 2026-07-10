# Repository Instructions

- This repository is a Rust workspace (`crates/lifeops-core`, `crates/lifeops-server`) plus a Svelte 5/Vite frontend in `frontend/`.
- Before coding, inspect the relevant plan/spec and nearby code. Prefer existing patterns over introducing new structure.
- For spec-driven or multi-step implementation work, keep `implementation.md` updated with assumptions, decisions, trade-offs, risks, verification results, and follow-up items. `implementation.md` is local-only and must remain ignored by Git.
- Do not revert unrelated local changes. Treat existing dirty files as user or prior-agent work unless explicitly told otherwise.
- Use focused tests first, then broader verification appropriate to the touched area:
  - Rust: `cargo test -p lifeops-core -p lifeops-server`, `cargo clippy -p lifeops-core -p lifeops-server -- -D warnings`
  - Frontend: from `frontend/`, `npm test`, `npm run check`, `npm run build`
- Avoid broad formatting churn. Format only files in scope when possible, and note any skipped existing formatting issues in `implementation.md`.
