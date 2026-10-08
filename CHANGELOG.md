# Changelog

All notable changes to Riku are listed here, newest first.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Riku is alpha: until 1.0, a minor version can change behavior and output.

## [Unreleased]

## [0.2.1] - 2026-10-08

### Added

- `riku about` shows the seal of the National University of Engineering (UNI, Lima, Peru), the version and the authors (Carlos Cueva and Amado Frias, electronic engineers). The interactive shell shows the same banner when it starts, instead of the RIKU wordmark. The seal is drawn as a Sixel image when the terminal supports it (Windows Terminal 1.22+, WezTerm, foot, Konsole, mlterm…), in maroon braille dots otherwise, or as text without colors; `RIKU_BANNER=sixel|braille|text|off` forces one.

### Changed

- The documentation is rewritten in English, for users (getting started, CLI reference, scripting, configuration, viewer, formats, LVS) and for contributors (architecture, development, design notes, roadmap).
- Installer messages (`get.sh`, `install.sh`) and the desktop entry are in English. The desktop entry keeps a Spanish translation.

### Fixed

- Help texts no longer claim that every command accepts `-f json`. They now list the commands that do: `diff`, `show`, `log`, `status`, `lvs` and `doctor`.
- The messages shown when Netgen is missing (in `riku lvs --netgen` and `riku doctor`) now say that `log --lvs` and `status --lvs` also need it. The viewer's LVS button no longer says it uses Netgen, since the manual links are the default.
- `riku lvs --cell` has its own help ("Layout cell to compare") instead of the one for images.
- `riku log --help` no longer shows an untranslated note next to `--color` values, and the examples of `riku demo --help` are printed one per line.
- The package description shown by `apt`/`dpkg` is in English.

## [0.2.0] - 2026-10-04

This release adds layout-versus-schematic (LVS) checks, both on their own and across history, extracts layout nets per cell with a cache, and adds two demo projects.

### Added

- **`riku lvs`** compares an Xschem schematic with its layout, either on disk or at a commit. Pairs come from `--sch`/`--layout`, from `[[lvs]]` entries in `.riku.toml`, or from matching names (`amp.sch` with `amp.gds`, `.oas` or `.mag`). Riku warns when a name matches several layouts.
  - **Manual links** (the default): which schematic transistor is which layout transistor, stored in `lvs/<cell>.toml` and versioned with the design. Riku checks models, parameters, pins, and the shorts and opens implied by the links, and says how much is left to link. `--suggest` adds the links that follow without guessing. `--update` rewrites positions after the layout moves. Links work across hierarchy, and survive a moved cell or sub-cell instance.
  - **Netgen** (`--netgen`), with Riku's own schematic netlister, so Xschem is not needed. `RIKU_LVS_KEEP` keeps the netlists and the Netgen report.
  - `-f json` output, CI exit codes with `--ci`, and `--log` to follow the LVS status over the last commits.
- **LVS in history.** `riku log --lvs` shows, per commit, whether the LVS broke, got worse, got better or was fixed, and which discrepancies appeared, changed or were resolved. `riku status --lvs` compares the working tree with `HEAD`, and its exit code works for pre-commit hooks.
- **LVS cache by dependencies.** A result is reused only if no file the run read has changed (including files it looked for and did not find), and the PDK, Netgen and Riku versions are the same.
- **LVS view in the viewer.** The schematic and the layout appear side by side, and each discrepancy is highlighted on both sides. A **Links** tab lets you link transistors by clicking them on each side.
- **Per-cell net extraction with memory.** Each cell is extracted once and stored by a hash of its content, in memory and on disk. An identical cell in another commit, on the other side of a diff or in a later run is not extracted again.
  - Whole chips can now be extracted: the 1 KB OpenRAM macro (127,628 transistors) takes a few seconds, where before nothing above 2 million polygons was extracted.
  - New environment variables: `RIKU_NETS_MEM_MB` (memory cap), `RIKU_HIER_INLINE` (size under which a sub-cell is merged into its parent), `RIKU_FULL_NETS` (compare whole chips that are not yet extracted) and `RIKU_FLAT_NETS` (go back to the flat extraction).
- **Demo projects:** `riku demo inversor`, a 5 V inverter in Magic and Xschem with 11 commits, a branch and the LVS at every commit. `riku demo chip` is a 1 KB OpenRAM SRAM macro with a bitcell change seen in thousands of instances, top-cell straps, a rename and a moved pin.
- **`riku log --graph --color auto|always|never`.**
- **`riku doctor`** reports whether Xschem and Netgen are installed.
- **Viewer:**
  - Navigate schematic hierarchy by double-clicking an instance, with a **Back** button.
  - The schematic diff dims what did not change.
  - Touchpad gestures (pinch and two-finger pan).
  - Tab in the History panel moves focus to the commit's files.

### Changed

- **Net and transistor diffs work per cell.** A short that only appears in a parent cell is reported once, in the lowest cell where it appears. An unnamed net is identified by the sub-cell it comes from. On the SRAM example, `riku log` drops from 22 s to 2.5 s and `riku show` from 9.5 s to 2.3 s, with half the memory.
- Cells above 2 million flattened polygons are compared per cell when their extraction is already cached, or with `RIKU_FULL_NETS=1`. Otherwise Riku warns and compares their sub-cells.
- The LVS layout netlist has no polygon cap any more.
- More of the viewer, the CLI and the format modules is translated. Spanish is available with `RIKU_LANG=es` or from the viewer's settings.

### Fixed

- Magic's `device … +types` rules and drain-extended transistors are evaluated. SKY130 5 V transistors now get the right model.
- A Magic port label applies to what lies under its rectangle, not to a point on its edge.
- An open in the top cell that falls over an unchanged instance now shows in the net diff.
- The viewer extracts nets with the same Magic port information as `riku lvs`, so net names agree between the two.
- In the LVS view, parallel devices that Netgen merges across different L are highlighted together, and two nets with the same label are treated as one, as Netgen does.
- In LVS history, a short no longer makes every parameter difference look fixed and then reappear. Netgen's `(no matching pin)` is no longer listed as a pin.
- `riku render` no longer draws layers that the viewer opens hidden.

## [0.1.0] - 2026-09-28

First release with the current version numbering. Versions were reset on 2026-09-28. Earlier pre-releases (0.1.0, 0.2.0 and 0.2.1, published in the days before) are superseded by this one.

### Added

- **Schematics (Xschem `.sch`):** added, removed and renamed components, and components with changed values, with each parameter before and after. Added and removed nets. A purely visual rearrangement (Move All) is recognized as cosmetic.
- **Layouts (GDSII, OASIS, Magic `.mag`):** which area changed, on which layer and in which cell, including changes from an instanced sub-cell. Magic layers by name, sub-cells from the same commit, and ports. Cell renames, metal fill, and cosmetic changes below an area threshold.
- **Transistors** in SKY130, GF180MCU and IHP SG13G2: model, W and L recognized from the PDK rules, and which ones changed.
- **Nets, opens and shorts** between two versions, with their location, including shorts that only appear in a parent cell. Fixing a short is reported as such.
- **Simulations (ngspice `.raw`):** which signals changed and by how much (maximum Δ, RMS, % of range), with numerical noise filtered out. Computed signals with ngspice syntax (`--expr`).
- **Commands:** `status`, `diff`, `show`, `log` (with `--graph`), `render`, `doctor`, `demo`, `completions`, `open` and `gui`, plus an interactive shell. Text or JSON output (`riku-diff/v2`, `riku-log/v2`, `riku-status/v2`) and exit codes for CI.
- **Desktop viewer:** both versions with an overlaid diff, the commit history and a whole-repository diff. In layouts there is a Transistors layer, the net under the cursor, click-to-highlight of a whole net, and a layer legend.
- **Demo projects:** `riku demo ota` (a SKY130 OTA with schematic, layout and simulation, 11 commits, a branch and a short that gets fixed) and `riku demo sram` (an OpenRAM SRAM with sub-cell changes, a rename and fill).
- English and Spanish (`RIKU_LANG=es`).
- A single Linux x86_64 executable that depends only on glibc, distributed as `.tar.gz` and `.deb` with an install script.

[Unreleased]: https://github.com/riku-chip/riku_chip/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/riku-chip/riku_chip/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/riku-chip/riku_chip/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/riku-chip/riku_chip/releases/tag/v0.1.0
