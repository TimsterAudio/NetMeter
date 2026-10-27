# TAW Net Meter: working conventions

The Net Meter (NM-001) is Timster's Audio Works' desktop network monitor: download and upload rates, Internet and LAN, as scrolling graphs in a small window. Rust with egui (via eframe). Its look (the `hifi` module: finishes, badge, window buttons and controls) comes from the shared `taw-panel` crate (TimsterAudio/TAW-Panel), pinned by commit in Cargo.toml. Change the shared look there, then move the pin. The window has no system title bar unless the user turns it on in Settings.

## Hard rules

- **Native, not HTML.** No web views or browser shells.
- **No network requests.** It only reads the computer's own counters. Fonts and the icon are built into the binary.
- **No special rights.** It must work as an ordinary user; anything that needs administrator or root is optional and says so.
- **Keep `rates.rs` and `lan.rs` free of UI and operating-system code**, and unit tested.
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

For panel changes, screenshot it under Xvfb at the default 420 × 180 and at the minimum 200 × 90, in Spool and Porcelain, light and dark (needs `libxkbcommon-x11-0`).

## Workflow

- Work on a feature branch. `main` always holds a working version.
- Open a pull request when the user asks, or merge when they ask.
- For every user-visible change, add a line under **Unreleased** in `CHANGELOG.md`.
- For a release: move Unreleased into a section headed `## X.Y.Z – YYYY-MM-DD`, and bump `version` in `Cargo.toml` and the version in `README.md`. Merging that to `main` publishes the release with the Linux and Windows packages (`.github/workflows/release.yml`).
