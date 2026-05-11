# Agent Notes

## Repo Shape
- Single-crate Rust CLI project (not a workspace).
- This repository is a Rust rewrite of `todoman`.
- Entry point is `src/main.rs`.
- Crate metadata and dependencies are in `Cargo.toml`.

## Verified Commands
- Build: `cargo build`
- Run CLI: `cargo run -- <args>`
- Check compile quickly: `cargo check`
- Run tests (when present): `cargo test`

## Tooling / Environment
- Nix flake is configured in `flake.nix`.
- `nix build` builds the crate via `naersk`.
- `nix run` runs the default package.
- `nix develop` provides `rustc` and `cargo` in the dev shell.

## Current State Gotchas
- README is only a placeholder; do not assume undocumented commands or architecture.
- There is currently no CI workflow or repo-local lint/format/typecheck config checked in.
- Keep changes simple and crate-local unless the user asks for larger structure.
