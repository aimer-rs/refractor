# Refractor — Development Instructions

Refractor is a code editor built with the Aimer framework. It lives beside the framework checkout, at
`../aimer`, which it depends on by path. These instructions apply to the entire repository. A nested
`AGENTS.md` takes precedence for files in its directory. The framework has its own `AGENTS.md`
(`../aimer/AGENTS.md`), which governs work done there.

## Critical Considerations

An editor is a performance-sensitive application: the user types, scrolls and selects all day, and every
keystroke has to feel instant, on a file of any size.

- When a request is vague, ask the user to clarify it.
- Treat per-frame rendering, layout, input, scrolling, highlighting and widget rebuild paths as hot unless
  evidence shows otherwise. A keystroke must cost the lines on screen, not the file.
- Avoid avoidable allocations, clones, repeated tree walks, blocking work and unnecessary synchronization
  on hot paths. Never copy the text of a file to answer a question about a few lines of it: read the part
  needed.
- Do not claim a change is faster without evidence. For non-obvious performance changes, run a benchmark,
  profiler, allocation check or an equivalent measurement, and report the numbers.
- Do not trade correctness or maintainability for speculative micro-optimizations.
- Assume phones are 3 to 10 times slower than the development machine.
- Strongly focus on the debug build: it is what is run and measured while developing. Do not lean on
  compiler optimization to make something fast; make the code do less. (The `[profile.dev.package.*]`
  entries in `Cargo.toml` are there at the app owner's request and are not a substitute.)
- Visual verification uses the Aimer MCP when it is available; see the `aimer-app` skill.

## Priorities

When requirements compete, use this order:

1. Correctness and safety. A text editor must never lose, reorder or corrupt the user's text.
2. The user's explicit request and acceptance criteria.
3. Performance on measured or clearly hot paths, in the debug build.
4. Consistency with the surrounding module and with `.codestyle`.
5. Minimal complexity, and few third-party dependencies.

Do not stop at a superficial workaround. Find and fix the root cause while keeping the change within the
requested scope.

## Additional Guidelines

- Prefer define the constant as assioated to a type for better ownership.
- Prefer `.expect("why it cannot fail")` over `.unwrap()`.
- Use `unchecked` methods only where the context makes undefined behavior impossible, and say why.
- Keep `unsafe` blocks minimal and document their safety invariants.
- Add comments for invariants and non-obvious decisions, not for code that is already self-explanatory.
- Do not run `cargo fmt`. Preserve the surrounding formatting style in edited code.

## Code style

Follow `.codestyle/*` (it is the reference for how widgets are organized: one widget per file, stateless and
stateful widget patterns, helpers, and borrowing over cloning). Where it is silent, match the surrounding
module.

- The app's visual theme and primary accent color is pink (`theme.marker` in `CodeTheme`, along with
  `theme.selected`, `theme.muted` and `theme.link`). Use `theme.marker` (and `theme.marker.darken(...)` or
  `theme.marker.with_alpha(...)`) for primary buttons, active tabs/toggles, checkboxes, highlights and
  interactive UI chrome. Reserve `theme.accent` (cyan) strictly for git-modified file names.

## Rust source layout

- Keep every project-owned `.rs` file below about 1,000 physical lines, tests and documentation comments
  excluded. When a file approaches the limit, move cohesive responsibilities into sibling modules and keep
  the parent as a small facade.
- Name modules after the responsibility they own. Re-export only what callers need.
- Keep tests beside the behavior they exercise, in a child `#[cfg(test)] mod tests` module.
- Prefer a flat layout, with at most one level of inner modules. When a module needs another level, it is
  too big: split it.

## Scope

- For requests to explain, review, diagnose or plan: inspect and report; do not modify code unless asked.
- For requests to build, change or fix: make the in-scope local edits and run the relevant non-destructive
  checks without asking first.
- Ask before destructive actions, external writes, adding a dependency, changing the framework's public API
  beyond the request, or materially expanding scope.
- Do not create commits, branches or pull requests, or publish artifacts, unless the user explicitly asks.

### Working in the framework

Refractor needs things the framework does not have yet (an editor surface that does not wrap, a gutter, a
syntax-aware highlighter). When the missing piece is a general capability and not something specific to this
app, build it in `../aimer`, under that repository's `AGENTS.md`, with its tests, and use it from here. Say
which repository each change is in when reporting. Never reach around the framework's public API from the
app.

## Test-Driven Development

Use red, green, refactor for behavior changes:

1. Add a focused test that fails for the intended reason.
2. Run it and confirm the failure.
3. Implement the smallest complete change that makes it pass.
4. Run the focused test again, then the crate's tests.
5. Refactor only while the tests remain green.

Additional rules:

- Bug fixes need a regression test.
- New behavior covers its happy path plus relevant invalid-input and boundary cases. For text, that
  includes the empty text, a text of one line and one of no final line break, CRLF, multi-byte characters,
  combining marks, and a very long line.
- Pure refactors may rely on existing tests when they cover the affected behavior.
- Keep tests deterministic. Fix time and random inputs.
- Never delete, ignore, weaken or skip a test merely to make the suite pass.
- If a pre-existing failure is unrelated, report it with evidence; do not silently work around it.
- If a useful test is impractical, say why before implementing and use the strongest feasible verification.

## Benchmarking and Optimization

- Never run benchmarks in parallel: they compete for resources and the results are invalid.
- Never game a benchmark; change the code, not the measurement.
- Run benchmarks in the debug build, with no `--release` and no `RUSTFLAGS`.
- Build the benchmark once, then run it, instead of rebuilding it each time.
- Use `criterion` for a benchmark where it is available, and keep benchmarks independent of each other:
  turn off any cache that would make them depend on one another.
- Report results relative to a baseline measured on the same machine, in the same session.

## Rust and Design Conventions

- Prefer clear ownership and borrowing over allocation, and zero-copy where it simplifies the code or
  measurably speeds up a hot path (see `.codestyle/code-organization-borrow-over-clone.md`).
- Use the latest stable Rust the framework supports.
- Prefer the framework and the standard library to a new dependency. A new third-party crate needs the
  user's approval; prefer one published more than one or two weeks ago, and pin it by version or, for a git
  dependency, by commit.
- Do not expose a third-party crate in a public signature.
- Add documentation comments to public items: purpose, invariants, panics and errors, and a short example
  where it helps. Keep them proportional to the item.
- Validate sizes, alignments, lifetimes and thread affinity when touching GPU buffers, FFI or platform code.

## Validation Commands

Run commands from the repository root. Prefer `cargo-nextest` when it is available.

```bash
# A focused test
cargo nextest run test_name

# All of this crate's tests
cargo nextest run

cargo check --tests

```

For the framework, run its own commands from `../aimer` (see its `AGENTS.md`).

Before handing off:

- Make sure production and test code compile.
- Run the narrowest relevant checks first, then broaden according to the risk of the change.
- For anything the user sees, check it in the running app (see below), not only in tests.
- Report exactly what was run and whether it passed, and what was not run and why.
- Summarize the changed behavior and name the remaining risks or follow-up work, without claiming
  anything unverified.

## Verification in the running app

Use the `aimer-app` skill and the Aimer MCP: restart the project after a change, take screenshots, and drive
the app with clicks, keys and scrolling. Prefer measuring a selection or a caret by reading it back (copy to
the clipboard and read it) over judging it from a picture. Do not kill processes by pattern; stop an app
only by the exact process id, because the user may be running their own sessions.
