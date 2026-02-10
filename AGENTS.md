# AGENTS Instructions

These rules apply to AI coding agents working in this repository.

## Code Organization Constraints
- Split code into reasonable modules; avoid piling most logic into one file.
- Keep each source file at or below `300` lines.
- Keep each function at or below `50` lines.

## Required Validation Before Handoff
- After making changes, you must run and pass:
  - `cargo check`
  - `cargo test`
  - `cargo clippy --all-targets --all-features`
- If any command fails or produces warnings, fix them before final delivery.

## Change Principles
- Prefer minimal, task-focused changes.
- If structure changes are needed to satisfy these constraints, preserve behavior compatibility and ensure all required validations pass.
