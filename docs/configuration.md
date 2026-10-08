# Configuration

How to configure Riku: the per-project `.riku.toml` file, environment variables, where Riku finds PDK files and Xschem symbols, the caches it keeps, and the language.

On this page:

- [Project file: `.riku.toml`](#project-file-rikutoml)
- [Environment variables](#environment-variables)
- [PDK and symbol discovery](#pdk-and-symbol-discovery)
- [Caches](#caches)
- [Language](#language)

## Project file: `.riku.toml`

A `.riku.toml` at the root of the repository fixes the comparison options for the project, so that everyone (people, CI and agents) compares the same way without repeating flags. Commit it with the design.

```toml
[layout]
cosmetic_threshold_um2 = 0.01        # µm²: a layout change with less total area is cosmetic

[waveform]
tolerance = "0.5%"                   # or 0.005: fraction of each signal's range
expressions = [                      # computed signals, always compared and drawn
  "gain = v(out)/v(in)",
  "tran: vpk = max(v(out))",
]

[[lvs]]                              # which schematic goes with which layout (repeatable)
schematic = "xschem/amp.sch"
layout = "layout/amp.gds"
cell = "amp"                         # optional: without it, the top cell
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `layout.cosmetic_threshold_um2` | number | `0.01` | Area in µm² under which a layout change is cosmetic. The default is below the minimum-area DRC rules of PDKs such as SKY130 and GF180 |
| `waveform.tolerance` | number or string | `0.1%` | Waveform tolerance as a fraction of each signal's range (`0.005`) or a percentage (`"0.5%"`). It must be greater than 0 and less than 1 (or 100%) |
| `waveform.expressions` | array of strings | empty | Computed signals in ngspice syntax, optionally prefixed by the analysis (`"ac: a0 = max(db(v(vout)))"`). See [formats.md](formats.md) |
| `[[lvs]]` `schematic`, `layout` | strings | — | A schematic/layout pair for `riku lvs`, `log --lvs` and `status --lvs`, with paths from the repository root |
| `[[lvs]]` `cell` | string | top cell | The layout cell to compare |

All sections and keys are optional. Without `[[lvs]]` entries, LVS pairs are found by name (`amp.sch` with `amp.gds`, `amp.oas` or `amp.mag`).

**Who reads it.** `riku diff`, `show`, `log`, `status`, `render` and `lvs`, and the desktop viewer. The file is looked up at the root of the repository that contains the folder given by `--repo` (or the current folder), not in subfolders.

**Precedence.** Command-line flags win over the file, and the file wins over each module's defaults. `--expr` expressions are **added** to the file's list (duplicates are dropped). `riku lvs --sch/--layout` replaces the `[[lvs]]` pairs.

**Errors.** The file is strict: a misspelled or unknown key is an error that names the key and lists the valid ones, not something silently ignored. An out-of-range tolerance is an error too. Every command that reads the file stops with that error, and `riku doctor` shows it in its "Git repository" section (and in `config.error` with `-f json`).

## Environment variables

### PDK and tools

| Variable | Effect |
|---|---|
| `PDK_ROOT` | Folder with the installed PDKs. Used for Xschem symbols, layer names and Magic technology data, Magic cell libraries and the Netgen setup. When unset, Riku also looks in `/foss/pdks` (the location used by iic-osic-tools) |
| `PDK` | The active PDK inside `PDK_ROOT` (for example `sky130A`). When unset, Riku picks the PDK by the symbols each schematic uses (see [below](#pdk-and-symbol-discovery)) |
| `PDKPATH` | Path of one PDK (`$PDK_ROOT/<pdk>`). Adds it to the PDKs Riku knows for layout technology data, and expands `$PDKPATH` in Magic `use` paths. When unset, `$PDKPATH` in a Magic file means `$PDK_ROOT/<tech>` |
| `TOOLS` | Root of an installation that contains Xschem; its standard symbol library (`$TOOLS/xschem/share/xschem/xschem_library/devices`) is used for symbols such as `res.sym` |
| `RIKU_MAG_PATH` | Extra folders, separated by `:`, where Riku looks for the `.mag` cells a Magic layout uses. Searched before the PDK libraries |
| `RIKU_MAG_LAMBDA` | Magic lambda in µm. Forces the value instead of reading it from the technology's `.tech` file. Only needed when the PDK of a `.mag` file is not installed and its technology is not SKY130, GF180 or IHP (Riku knows those) |

In iic-osic-tools, `sak-pdk <name>` sets `PDK_ROOT`, `PDK` and `PDKPATH` for you.

### Behavior and output

| Variable | Effect |
|---|---|
| `RIKU_LANG` | Language of the CLI and the viewer: `en` (default) or `es`. Values like `es_PE.UTF-8` count as `es`; an unknown value falls back to English. See [Language](#language) |
| `RIKU_JOBS` | Threads for heavy work, like `--jobs N` (the flag wins). Default: the available cores. `1` runs everything on one thread |
| `RIKU_NO_CACHE` | Any non-empty value other than `0` disables every on-disk cache: layout diffs, layout nets and LVS results |
| `RIKU_CACHE_DIR` | Where to keep the caches instead of the system cache folder (see [Caches](#caches)) |
| `RIKU_ASCII` | Non-empty and not `0`: `riku log --graph` draws with ASCII characters (like `--ascii`) |
| `NO_COLOR` | Non-empty and not `0`: no colors in `riku log --graph` (unless `--color always`), and the shell banner as text |
| `RIKU_BANNER` | The banner of the shell and of `riku about`: `sixel` (the seal as an image), `braille` (in braille dots), `text` (the credits only) or `off` (nothing in the shell). Default: the best the terminal can draw (see [`riku about`](cli.md#riku-about)) |
| `CLICOLOR_FORCE` | Non-empty and not `0`: colors even when the output is not a terminal (unless `--color never`). It wins over `NO_COLOR` |
| `RIKU_FULL_NETS` | Non-empty and not `0`: also compare nets and transistors in layout cells larger than 2 million flattened polygons that have not been extracted before. Without it, those cells skip the net and transistor comparison the first time, which keeps a whole-chip diff fast; the geometry is always compared |
| `RIKU_LVS_KEEP` | A folder. After each Netgen run, Riku copies there the two netlists (`schematic.spice`, `layout.spice`) and Netgen's reports (`comp.out`, `comp.json`), to inspect them by hand |

### Read from the system

| Variable | Used for |
|---|---|
| `HOME` | `~/riku-demos` (the default of `riku demo`), `~/.xschemrc`, `~` in Magic paths and the default cache folder |
| `XDG_CACHE_HOME` | The system cache folder, when set (otherwise `~/.cache`) |
| `PATH` | Where `netgen` is looked up (then `/foss/tools/bin`) |
| `DISPLAY`, `WAYLAND_DISPLAY` | Whether the desktop viewer can open (Linux) |

Variables used only for developing Riku itself (profiling, the viewer's level of detail, alternative net extraction modes and memory limits) are described in [dev/development.md](dev/development.md#development-environment-variables).

## PDK and symbol discovery

### Xschem symbols

To draw a schematic and connect its pins, Riku needs the `.sym` files its instances use. It looks for each symbol in this order, using only folders that exist:

1. **The project.** A symbol next to the schematic or from the repository root, taken from **the same version** as the schematic: in a diff between two commits, each side uses its own symbols. The viewer, images (`riku render`, `-f png|svg`) and the LVS netlist use this step; the text and JSON diff start at step 2, so keep project symbols reachable through `.xschemrc` if their pins matter for net comparison.
2. **`.xschemrc`** in the current folder, or else in your home folder. Riku reads `set PDK_ROOT` with `set PDK`, `set XSCHEM_SHAREDIR` and `append XSCHEM_LIBRARY_PATH` lines from it.
3. **Environment variables.** `$PDK_ROOT/$PDK/libs.tech/xschem`, and `$TOOLS/xschem/share/xschem/xschem_library/devices`.
4. **Detection.** When `$PDK` is not set, Riku picks among the PDKs installed in `$PDK_ROOT` (or `/foss/pdks`) the one that has the most of the symbols the schematic references (`sky130_fd_pr/nfet_01v8.sym` points to `sky130A`). A design that mixes PDKs gets all the ones it needs. On a tie, `sky130A`, `gf180mcuD` and `ihp-sg13g2` are preferred over their variants.

When `$PDK` is set but a schematic uses symbols from another installed PDK (for example an older design after switching PDKs), the missing symbols are taken from the PDK that has them.

A symbol that cannot be found is drawn as an empty box and its pins do not connect, which affects net comparison. The viewer says where the PDK came from and lists missing symbols in its Details panel; `riku doctor` summarizes the sources:

```text
--- PDK ---
  [--]  .xschemrc: not found
  [ok]  $PDK_ROOT/$PDK → …/pdks/gf180mcuD/libs.tech/xschem
  [ok]  $TOOLS → …/tools/xschem/share/xschem/xschem_library/devices
  [ok]  Magic: .mag libraries in gf180mcuD (5), sky130A (6)
```

### Layouts

- **Layer names and technology data** come from the PDKs installed in `$PDK_ROOT` (plus `$PDKPATH`), with `$PDK` first. For a GDS or OASIS file, Riku picks the PDK whose layer map knows most of the file's layers.
- **Magic sub-cells** (`use` lines) are looked up, from the same version as the file that uses them: in the folder the `use` line gives (relative, or with `$PDK_ROOT`, `$PDKPATH` or `~`), next to the file, in `$RIKU_MAG_PATH`, and in the PDK's cell libraries (`$PDK_ROOT/<tech>/libs.ref/*/mag`). The first folder that has a cell wins. A cell that cannot be found is left empty with a warning, and the rest is compared.
- **Magic lambda** is read from the technology's `.tech` file in the PDK; without it, Riku uses the known values of SKY130, GF180 and IHP, or `RIKU_MAG_LAMBDA`.

### LVS

- The schematic netlist uses the variables of the PDK's `libs.tech/xschem/xschemrc` (for model includes).
- Netgen runs with the PDK's setup file, `libs.tech/netgen/<pdk>_setup.tcl`. A PDK without it cannot be compared with `--netgen`.
- `netgen` is looked up in `PATH`, then in `/foss/tools/bin`. It is optional: only `riku lvs --netgen`, `log --lvs` and `status --lvs` need it. `riku doctor` says whether it was found.

## Caches

Riku keeps several caches so that repeating a comparison (another `riku diff`, a `riku log`, reopening a diff in the viewer, rerunning the LVS) does not recompute what did not change. All of them are safe to delete at any time; a damaged or outdated entry is simply recomputed.

| Cache | Default location | What it holds | Limit |
|---|---|---|---|
| Layout diffs | `~/.cache/riku/diff` | Results of layout comparisons whose inputs add up to more than 1 MiB | 512 MiB; the oldest entries are removed |
| Layout nets | `~/.cache/riku/nets` | Per-cell net and transistor summaries that took a while to build | 512 MiB; the oldest entries are removed |
| LVS (Netgen) | `~/.cache/riku/lvs/v2` | Netgen results, reused for a version only if every file the run read has the same content and the environment (PDK, Netgen) is the same | 200 entries per pair |
| LVS (manual links) | `~/.cache/riku/lvs/manual-v1` | The extracted transistors and nets of each side | 200 entries per pair |

`~/.cache` is the system cache folder (`$XDG_CACHE_HOME` when set).

With `RIKU_CACHE_DIR=<dir>`, the layout diff entries are stored directly in `<dir>`, the nets in `<dir>/nets` and the LVS caches in `<dir>/lvs`.

| To disable | Scope |
|---|---|
| `--no-cache` (on `diff` and `show`) | The layout diff and layout nets caches, for that command |
| `RIKU_NO_CACHE=1` | Every on-disk cache, for every command and the viewer |

Commit-based entries are validated against Git content, so switching branches or rewriting history never returns a stale result.

Images written by `riku render` and `-f png|svg` without `-o` go to the `riku` folder inside the system temp folder (`/tmp/riku` on Linux). They are not a cache: Riku does not clean them up.

## Language

The command line, its help and the viewer are in English by default. Spanish is available:

- **Command line:** set `RIKU_LANG=es` (for example `export RIKU_LANG=es` in your shell profile). The value is read at startup; `es_PE.UTF-8` and `es-PE` count as `es`, and an unknown value means English.
- **Viewer:** choose it in **Settings → Language**; the choice is remembered. `RIKU_LANG`, when set to a supported language, wins over the saved choice.

JSON keys and enum values are the same in every language; free-text messages (errors, warnings) follow the language. Adding a translation is explained in [dev/development.md](dev/development.md#translations).
