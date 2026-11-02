# TAW Net Meter: working conventions

The Net Meter (NM-001) is Timster's Audio Works' desktop network monitor: download and upload rates, Internet and LAN, as scrolling graphs in a small window, which expands into a basic system dashboard (CPU, memory, disks, GPU, temperatures, USB). Rust with egui (via eframe). Its look is **Glass**, from TAW-Panel's looks review (`docs/looks-review.html`, decided 2026-10-27): `src/glass.rs` holds it until a second utility needs it, then it moves into the shared `taw-panel` crate (TimsterAudio/TAW-Panel) beside `hifi`. The app still uses `taw-panel` (pinned by commit in Cargo.toml) for window resizing and the 0.1.0 settings type. The window is see-through, has no system title bar unless the user turns it on in Settings, and shows its controls only under the mouse; settings open in their own window.

## Hard rules

- **Native, not HTML.** No web views or browser shells.
- **No network requests.** It only reads the computer's own counters. Fonts and the icon are built into the binary.
- **No special rights.** It must work as an ordinary user; anything that needs administrator or root is optional and says so.
- **Keep `rates.rs`, `lan.rs` and `metrics.rs` free of UI and operating-system code**, and unit tested. Each system's code goes in `net/` and `sys/`, and every reading is optional: a tile with nothing to show stays hidden.
- **Never break saved settings.** `Saved` in `app.rs` uses `#[serde(default)]`. Add new fields with defaults, and never rename or repurpose existing ones.

## Files

See the table in README.md.

## Checks before pushing

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy --target x86_64-pc-windows-gnu --all-targets -- -D warnings   # Windows still compiles
```

For panel changes, screenshot it under Xvfb at the default 420 × 180 and at the minimum 200 × 90, frosted and smoked, with the mouse over it and not (needs `libxkbcommon-x11-0`; run `xcompmgr` and put a picture behind the window to see the glass).

## Workflow

- Work on a feature branch. `main` always holds a working version.
- Open a pull request when the user asks, or merge when they ask.
- For every user-visible change, add a line under **Unreleased** in `CHANGELOG.md`.
- For a release: move Unreleased into a section headed `## X.Y.Z – YYYY-MM-DD`, and bump `version` in `Cargo.toml` and the version in `README.md`. Merging that to `main` publishes the release with the Linux and Windows packages (`.github/workflows/release.yml`).
