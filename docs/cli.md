# Command-line reference

Every `riku` command, its options and its text output. This page covers text output. For JSON, exit codes and CI, see [scripting.md](scripting.md). For `.riku.toml`, environment variables and caches, see [configuration.md](configuration.md).

On this page:

- [Commands at a glance](#commands-at-a-glance)
- [Conventions](#conventions)
- [Interactive shell](#interactive-shell)
- [`riku status`](#riku-status)
- [`riku diff`](#riku-diff)
- [`riku show`](#riku-show)
- [`riku log`](#riku-log)
- [Images: `riku render` and `-f png|svg`](#images-riku-render-and--f-pngsvg)
- [`riku lvs`](#riku-lvs)
- [`riku open` and `riku gui`](#riku-open-and-riku-gui)
- [`riku doctor`](#riku-doctor)
- [`riku demo`](#riku-demo)
- [`riku completions`](#riku-completions)
- [Exit codes](#exit-codes)

The examples use the bundled demo projects. To follow along, run `riku demo` (it creates them in `~/riku-demos`) and `cd ~/riku-demos/ota`.

## Commands at a glance

| Command | What it does |
|---|---|
| `riku` | Opens the [interactive shell](#interactive-shell) |
| `riku status` | Summarizes what changed in the working tree since `HEAD`, file by file |
| `riku diff [A] [B] [FILE]` | Semantic changes between two versions (commits or the working tree), of one file or all of them |
| `riku show COMMIT [FILE]` | Changes of a commit against its first parent |
| `riku log [FILE]` | History with a semantic summary per commit, optionally with the branch graph and LVS |
| `riku render FILE` | Draws one version of a file as a PNG or SVG image, without a window |
| `riku lvs [REV]` | Checks a layout against its schematic (layout-vs-schematic) |
| `riku open [FILE]` | Opens the desktop viewer and returns the terminal |
| `riku gui [FILE]` | Runs the desktop viewer in this process |
| `riku doctor` | Checks the environment and lists the formats this `riku` can compare |
| `riku demo [NAME]` | Creates example projects with history to try Riku |
| `riku completions SHELL` | Prints a shell completion script |

## Conventions

**Help.** Every command accepts `--help` (or `riku help <command>`), with examples. `riku --version` prints the version.

**Repository.** Commands that read Git (`status`, `diff`, `show`, `log`, `render`, `lvs`, `doctor`) take `-r, --repo <REPO>`; the default is the current folder. Riku finds the repository root from there, as Git does.

**File paths.** You name files from where you are, as in Git: inside `repo/xschem`, `riku diff ota-5t.sch` means `xschem/ota-5t.sch`. A path from the repository root also works if it exists. This applies to `diff`, `show`, `log`, `render --rev`, `lvs --sch/--layout` and the shell.

**Threads.** `--jobs N` is accepted by every command and sets the threads used for heavy work (layout diff, viewer loading). The default is the number of available cores; `RIKU_JOBS` sets it from the environment. `--jobs 1` (or `RIKU_JOBS=1`) runs everything on one thread.

**Formats.** Riku compares Xschem schematics (`.sch`), layouts (`.gds`, `.oas`, `.mag`) and ngspice results (`.raw`). A file that no module recognizes is listed without a diff. What each format compares is in [formats.md](formats.md).

**Language.** Output and help are in English. `RIKU_LANG=es` switches to Spanish (see [configuration.md](configuration.md#language)).

**JSON.** `status`, `diff`, `show`, `log`, `lvs` and `doctor` accept `-f json`, with a versioned `schema` field. The schemas are documented in [scripting.md](scripting.md).

**Project options.** A `.riku.toml` at the repository root sets the cosmetic threshold, waveform tolerance, expressions and LVS pairs for everyone. Command-line flags win over the file. See [configuration.md](configuration.md#project-file-rikutoml).

## Interactive shell

`riku` with no arguments opens a shell. The prompt shows the current folder and whether it is inside a Git repository:

```text
riku ota (git)> status
riku ota (git)> diff HEAD~1 HEAD xschem/ota-5t.sch
riku ota (git)> cd layout
riku layout (git)> log ota-5t.gds -n 5
```

- Every command works without the `riku` prefix, with the same options and output.
- Extra commands: `ls [path]` lists folders and the files Riku can open (marked `[git]` if they are inside the repository), `cd <path>` changes folder, `help` lists the commands, and `exit`, `quit` or `q` leave. Ctrl+D and Ctrl+C also leave.
- **Tab** completes commands, flags, folders, design files, local branches, tags, `HEAD` and recent commit hashes. The up and down arrows walk the history.
- Lines are split like a POSIX shell: use quotes for paths with spaces and for expressions, for example `diff v0.1 v1.0 sim/ota-5t_tb.raw --expr "ac: a0 = max(db(v(vout)))"` or `cd "my folder"`.
- File arguments are relative to the shell's current folder, and `--repo` defaults to the repository that folder belongs to.
- `--jobs` is fixed when the shell starts. Passing it to a command inside the shell has no effect.
- The shell needs a terminal. When standard input is not a terminal (a script, a pipe, an agent), `riku` with no arguments prints the help and exits with 0.

## `riku status`

```bash
riku status [--detail | --full] [-f text|json] [--compact] [--paths PAT]... [--include-unknown] [--lvs] [-r REPO]
```

Compares every modified file in the working tree against `HEAD` and puts it in one category:

| Category | Meaning |
|---|---|
| `semantic` | At least one functional change |
| `cosmetic` | Only cosmetic changes (repositioning, area under the threshold, waveforms within tolerance) |
| `unchanged` | The file changed on disk but the module finds no difference |
| `unknown` | No module for this format; counted, and listed with `--include-unknown` |
| `error` | Could not be compared (a damaged file, or one larger than 50 MB); the message is printed |

```text
$ riku status
On branch scratch (HEAD 421f72d)

Modified with semantic changes:
  xschem/ota-5t.sch    2 components modified

Not recognized by Riku (1): use --include-unknown to list them.
```

| Option | Effect |
|---|---|
| `--detail` | One line per changed component, net or signal |
| `--full` | The full report of each module per file (implies the detail) |
| `--paths PAT` | Only files matching the glob (repeatable), e.g. `--paths 'xschem/*.sch'` |
| `--include-unknown` | Also list the files no module recognizes |
| `-f json`, `--compact` | JSON output (`riku-status/v2`), indented or on one line |
| `--lvs` | Add the LVS of each schematic/layout pair, working tree against `HEAD` (below) |
| `--ci` | Accepted for uniformity; it has no effect because `status` always uses CI exit codes |

The header also shows the upstream branch and how far ahead or behind you are, when there is one. Module warnings (for example a Magic sub-cell that cannot be found) are printed under the file.

**Exit code:** 0 when nothing changed or only cosmetic changes, 1 when there are functional changes, 2 on error (including any file in the `error` category).

### LVS in `status`: `--lvs`

`--lvs` runs the Netgen LVS of every schematic/layout pair on `HEAD` and on the working tree, and reports how the result moved. It needs `netgen` (see [lvs.md](lvs.md)).

```text
$ riku status --lvs
LVS: 1 new Netgen run(s); the rest from the cache
On branch scratch (HEAD 421f72d)

Modified with semantic changes:
  xschem/ota-5t.sch    2 components modified

Not recognized by Riku (1): use --include-unknown to list them.

LVS (HEAD → working tree)
  ota-5t        different parameters
                ~ M1 w: schematic 4 → 6, layout 2 (−67 %)
                ~ M2 w: schematic 4 → 6, layout 2 (−67 %)
```

With `--lvs`, **the exit code is the LVS one**, which makes it a good pre-commit hook:

| Case | Exit code |
|---|---|
| A pair stopped matching, or got worse (the connections no longer match) | 1 |
| Error (Netgen missing, a netlist that cannot be built) | 2 |
| Same or better | 0 |
| Same verdict, but new discrepancies appeared | 0, with a warning on stderr |

In the example above the verdict is still "different parameters", so the exit code is 0 even though the schematic changed. If no pair is found, a warning is printed and the LVS part is skipped.

## `riku diff`

```bash
riku diff [A] [B] [FILE] [-f text|json|visual|png|svg] [--compact] [--ci]
          [--cosmetic-threshold-um2 UM2] [--tolerance TOL] [--expr EXPR]... [--no-cache]
          [-o FILE] [--cell CELL] [--size WxH] [--theme light|dark] [-r REPO]
```

Like `git diff`: without `B` the comparison is against the **working tree** (the files on disk, including uncommitted changes); without `A`, against `HEAD`; without a file, every file that changed.

| Form | Compares |
|---|---|
| `riku diff` | Working tree against `HEAD`, every file |
| `riku diff amp.sch` | That file, working tree against `HEAD` |
| `riku diff main` | Working tree against `main`, every file |
| `riku diff main amp.sch` | That file, working tree against `main` |
| `riku diff HEAD~1 HEAD` | Everything that changed between two commits |
| `riku diff HEAD~1 HEAD amp.sch` | That file between two commits |

An argument is a **file** if a module knows its extension or it exists on disk; otherwise it is a **revision** (hash, branch, tag, `HEAD~2`). At most three arguments are accepted.

### Text output

Without a file, the output starts with the two versions and the number of files, followed by each file. Files without a module are listed at the end:

```text
$ riku diff
diff HEAD → worktree  (2 files)

File     : xschem/ota-5t.sch
Changes  : 2

  ~ M1
      W: 4 → 6
  ~ M2
      W: 4 → 6

No Riku module (1): lvs/ota-5t.toml
```

Each change starts with a marker:

| Marker | Meaning |
|---|---|
| `+` | Added |
| `-` | Removed |
| `~` | Modified |
| `r` | Renamed |
| `!` | A layout open or short: it changes the circuit. Listed first |

`Cosmetic : N` counts the cosmetic changes, which are not listed. A new or deleted file lists everything as added or removed.

A schematic between two tags of the `ota` demo (trimmed):

```text
$ riku diff v0.1 v1.0
diff v0.1 → v1.0  (4 files)
…
File     : xschem/ota-5t.sch
Changes  : 7
Cosmetic : 21

  ~ M1
      W: 2 → 4
  ~ M2
      W: 2 → 4
  ~ M3
      W: 20 → 18
  ~ M4
      W: 20 → 18
  ~ p17
      lab: node → tail
  ~ p18
      lab: node → tail
  ~ p19
      lab: node → tail

  + net:tail
  - net:node

File     : xschem/ota-5t_tb.sch
Changes  : 2

  ~ C1
      value: 1p → 2p
  ~ x1
      changed inside: xschem/ota-5t.sch
```

A layout change is named `cell:layer` (`L<layer>/<datatype>`, or the layer name in Magic), a transistor `cell:model @ (x, y)` and a layout net `cell:net:name`. Areas are in µm², coordinates in µm:

```text
$ riku show 120ee0b
…
File     : layout/ota-5t.gds
Changes  : 2

  ! ota-5t:net:Vout = Vp
      short (nets joined): Vout, Vp → Vout = Vp
      bbox: (-5.600, -0.560) → (-0.400, 9.550) µm
  + ota-5t:L70/20
      +1 polys / +6.716 µm²
      -0 polys / -0.000 µm²
      bbox: (-5.600, -0.560) → (-0.400, 9.550) µm
```

A simulation result lists each signal that changed beyond the tolerance, with the largest difference, where it happens, the RMS difference and the share of the signal's range. With `--expr`, computed signals are compared too:

```bash
riku diff v0.1 HEAD sim/ota-5t_tb.raw --expr "ac: a0 = max(db(v(vout)))"
```

```text
File     : sim/ota-5t_tb.raw
Changes  : 76
Cosmetic : 51

  ~ @m.x1.xm1.msky130_fd_pr__pfet_01v8[gds]
      Δmax 31.285 n at 1.800 V · RMS 31.285 n · 28.95 % of range  (Operating Point)
…
  ~ v(vout)
      Δmax 6.287 dB at 100.000 MHz · RMS 6.101 dB · 10.15 % of range  (AC Analysis)
…
  ~ a0
      = max(db(v(vout)))
      38.292 dB → 38.881 dB · Δ 0.589 dB (1.51 %)  (AC Analysis)
```

What each format reports, and what counts as cosmetic, is in [formats.md](formats.md).

### Options

| Option | Effect |
|---|---|
| `-f, --format` | `text` (default), `json` (`riku-diff/v2` for one file, `riku-diff-set/v1` without a file), `visual` (opens the viewer), `png` or `svg` (an image, see [Images](#images-riku-render-and--f-pngsvg)) |
| `--compact` | JSON on one line instead of indented |
| `--ci` | CI exit codes (see [Exit codes](#exit-codes)) |
| `--cosmetic-threshold-um2 UM2` | Area in µm² under which a layout change is cosmetic. Default 0.01 µm², or the value in `.riku.toml`. Other formats ignore it |
| `--tolerance TOL` | Waveform tolerance, as a fraction of each signal's range (`0.005`) or a percentage (`0.5%`). Default `0.1%`, or the value in `.riku.toml`. Must be greater than 0 and less than 1 |
| `--expr EXPR` | A computed signal to compare in `.raw` files (repeatable), in ngspice syntax: `"v(out)/v(in)"`, `"gain = db(v(out))"`, `"tran: vpk = max(v(out))"`. Added to the expressions in `.riku.toml`. See [formats.md](formats.md) |
| `--no-cache` | Do not read or write the on-disk layout caches (same as `RIKU_NO_CACHE=1`) |
| `-o, --output`, `--cell`, `--size`, `--theme` | Image options, used with `-f png|svg` |
| `-r, --repo` | Repository (default: the current folder) |

### In the viewer: `-f visual`

`-f visual` opens the desktop viewer and returns the terminal. With a file, it opens that diff (Diff, Before and After views). Without a file, it opens the list of every file that changed between A and B; a click opens each diff. When B is the working tree, the viewer labels it `worktree`. See [viewer.md](viewer.md).

## `riku show`

```bash
riku show COMMIT [FILE] [same options as diff]
```

Like `git show`, but semantic: the changes of a commit against its first parent, file by file.

```text
$ riku show 421f72d
commit 421f72d  (parent d4e936d)
Author : Riku Demo
Date   : 2026-03-08 08:30

    Rename the tail node: node -> tail

File     : xschem/ota-5t.sch
Changes  : 3

  ~ p17
      lab: node → tail
  ~ p18
      lab: node → tail
  ~ p19
      lab: node → tail

  + net:tail
  - net:node
```

- With a file, only that file: `riku show abc123 amp.sch` is the same as `riku diff abc123~1 abc123 amp.sch`.
- The initial commit is compared against an empty tree (everything appears added). A merge is compared against its first parent.
- A file with only cosmetic changes prints `no semantic changes`. Files without a module are listed at the end.
- It accepts every option of `diff`. `-f json` uses the `riku-show/v1` schema. `-f visual` without a file opens the list of files the commit changed. The initial commit has no parent to compare with, so `-f visual` is refused there; use `riku open FILE` or `riku render FILE --rev COMMIT` to look at it.

## `riku log`

```bash
riku log [FILE] [-n N] [--detail | --full] [-f text|json] [--compact] [--paths PAT]...
         [--branch REF] [--graph [--ascii] [--color auto|always|never]] [--lvs] [-r REPO]
```

The last 20 commits (or `-n N`), with their refs (branches, tags, `HEAD`) and, for each file with a module, a summary of what changed against the first parent:

```text
$ riku log -n 5
* 421f72d (HEAD, main, v1.0)  Rename the tail node: node -> tail
          Riku Demo · 2026-03-08 08:30
          xschem/ota-5t.sch  3 components modified, 1 net added, 1 net removed

* d4e936d  Testbench: 2 pF load
          Riku Demo · 2026-03-07 19:30
          sim/ota-5t_tb.raw  38 signals changed
          xschem/ota-5t_tb.sch  1 component modified

* 6184836  Fix: the Vout route no longer touches Vp
          Riku Demo · 2026-03-07 06:30
          layout/ota-5t.gds  1 component removed, 1 net modified

* 120ee0b  Layout: route Vout to the left edge
          Riku Demo · 2026-03-06 17:30
          layout/ota-5t.gds  1 short, 1 component added

* ad104a7 [merge]  Merge branch 'narrow-input-pair'
          Riku Demo · 2026-03-06 04:30
          (merge commit; no per-file diff)
```

Shorts and opens are listed first in each summary. A file with only cosmetic changes shows `(cosmetic changes only)`, and a merge is marked `[merge]` without a per-file diff.

| Option | Effect |
|---|---|
| `FILE` | Only commits that touch this file (the same as `--paths` with that path) |
| `-n, --limit N` | How many commits (default 20) |
| `--detail` | One line per changed component, net or signal |
| `--full` | The full report of each module per file |
| `--paths PAT` | Only commits that touch files matching the glob (repeatable) |
| `--branch REF` | Start from another branch, tag or commit instead of `HEAD` |
| `--graph` | Draw the branch and merge graph on the left |
| `--ascii` | With `--graph`: ASCII characters instead of Unicode |
| `--color WHEN` | With `--graph`: `auto` (default), `always` or `never` |
| `--lvs` | Add the LVS of each schematic/layout pair to each commit (below) |
| `-f json`, `--compact` | JSON output (`riku-log/v2`), indented or on one line |

With `--paths` (or a file), `-n` counts only the commits that touch those files against their first parent, like `git log -n N -- file`. A merge that does not touch them is not shown either.

### The branch graph: `--graph`

`--graph` lists commits in topological order (each commit before its parents) and draws the branches and merges, like `git log --graph`:

```text
$ riku log --graph
● 421f72d (HEAD, main, v1.0)  Rename the tail node: node -> tail
│   Riku Demo · 2026-03-08 08:30
│   xschem/ota-5t.sch  3 components modified, 1 net added, 1 net removed
…
○   ad104a7 [merge]  Merge branch 'narrow-input-pair'
│     Riku Demo · 2026-03-06 04:30
├─╮
● │ 6568371  Tidy up the schematic (move everything)
│ │   Riku Demo · 2026-03-05 15:30
│ │   xschem/ota-5t.sch  (cosmetic changes only)
● │ e64f5f2  Layout: metal5 power straps on VDD and VSS
│ │   Riku Demo · 2026-03-05 02:30
│ │   layout/ota-5t.gds  2 components added
│ ● 02496a4 (narrow-input-pair)  Layout: trim the input pair diffusion to match
│ │   Riku Demo · 2026-03-04 13:30
│ │   layout/ota-5t.gds  6 components modified, 1 component removed
│ ● 0f05efe  Narrower input pair: M3, M4 W 20u -> 18u
│ │   Riku Demo · 2026-03-04 00:30
│ │   sim/ota-5t_tb.raw  71 signals changed
│ │   xschem/ota-5t.sch  2 components modified
├─╯
● a603147  Wider PMOS load: M1, M2 W 2u -> 4u
…
```

- `●` is a commit, `○` a merge, and `┆` a branch that continues beyond `-n`.
- Each branch gets a color when the output is a terminal. `NO_COLOR` turns colors off and `CLICOLOR_FORCE=1` forces them. `--color always|never` decides regardless of the environment and wins over both: `always` is useful with `less -R` or in CI logs, `never` for pasting into a text. JSON never has color.
- `--ascii` (or `RIKU_ASCII=1`) draws with `* | / \ -` for terminals or fonts without Unicode.
- With `--paths`, commits that do not touch those files are hidden and their children connect to the nearest visible ancestor.
- With `-f json`, each commit carries its place in the graph (see [scripting.md](scripting.md#riku-logv2)).

### LVS in the history: `--lvs`

`--lvs` adds, for each schematic/layout pair (the same pairs as [`riku lvs`](#riku-lvs)), the Netgen LVS verdict of each commit, how it moved against the first parent, and which discrepancies appeared (`+`), changed value (`~`) or were fixed (`−`):

```text
$ riku log --lvs -n 12
LVS: 10 new Netgen run(s); the rest from the cache
…
* 6184836  Fix: the Vout route no longer touches Vp
          Riku Demo · 2026-03-07 06:30
          layout/ota-5t.gds  1 component removed, 1 net modified
          LVS ota-5t: different parameters  ← got better: the connections match again
            − unmatched nets: Vout, Vp (layout: Vout)
            − pin Vp only in the schematic

* 120ee0b  Layout: route Vout to the left edge
          Riku Demo · 2026-03-06 17:30
          layout/ota-5t.gds  1 short, 1 component added
          LVS ota-5t: do NOT match  ← got worse: the connections no longer match
            + unmatched nets: Vout, Vp (layout: Vout)
            + pin Vp only in the schematic
…
* 02496a4 (narrow-input-pair)  Layout: trim the input pair diffusion to match
          Riku Demo · 2026-03-04 13:30
          layout/ota-5t.gds  6 components modified, 1 component removed
          LVS ota-5t: different parameters
            + M8 w: schematic 20, layout 19 (−5.0 %)
            ~ M3 w: schematic 18, layout 20 → 19 (+5.6 %)
            ~ M4 w: schematic 18, layout 20 → 19 (+5.6 %)
…
* a603147  Wider PMOS load: M1, M2 W 2u -> 4u
          Riku Demo · 2026-03-03 11:30
          sim/ota-5t_tb.raw  65 signals changed
          xschem/ota-5t.sch  2 components modified
          LVS ota-5t: different parameters  ← stopped matching
            + M1 w: schematic 4, layout 2 (−50 %)
            + M2 w: schematic 4, layout 2 (−50 %)

* 5338fc4 (v0.1)  Initial OTA: schematic, testbench, layout and simulation
          Riku Demo · 2026-03-02 22:30
          (root commit)
          LVS ota-5t: match
```

- Transitions: `← stopped matching`, `← got worse`, `← got better`, `← matches again`.
- Discrepancies are matched across versions by the **schematic** names (`M3 w`); layout device names are indices that shift. The percentage is the layout's error against the schematic, which is the intent.
- When the connections do not match in either version (a short, an open), Netgen does not compare parameters, so only nets, devices and pins are counted.
- Without `--detail`, at most 3 discrepancy lines per commit (`… and N more`), and nothing on commits where the LVS did not change. `--detail` shows all of them; `--full` also lists every discrepancy of each commit.
- Results are cached: Netgen only runs for what was not computed before, and the count of new runs is printed on stderr. Without Netgen the log still prints, with a warning.

## Images: `riku render` and `-f png|svg`

Riku draws images without opening a window (no display or GPU needed), which works in CI, over SSH and for AI agents that can look at images. They look like the viewer: schematics and layouts with the diff colors, and waveforms with A dashed, B solid and the B − A error.

```bash
riku render xschem/ota-5t.sch                              # the file on disk, as PNG
riku render layout/ota-5t.gds --rev v0.1 -f svg -o ota.svg # a committed version, as SVG
riku render sim/ota-5t_tb.raw --expr "ac: a0 = max(db(v(vout)))"
riku diff HEAD~1 HEAD xschem/ota-5t.sch -f png             # an image of a diff
riku show 120ee0b layout/ota-5t.gds -f png --theme dark    # what a commit changed
```

```text
$ riku render xschem/ota-5t.sch -o /tmp/rikucap/ota.png
/tmp/rikucap/ota.png
```

- **`riku render FILE [--rev REV]`** draws one version: the file on disk (the path as written, no repository needed) or the version in a commit with `--rev`.
- **`diff` and `show` with `-f png|svg`** draw the diff of one file. A file is required.
- **Output:** the absolute path of the image is printed on stdout. Without `-o`, it goes to the `riku` folder inside the system temp folder (`/tmp/riku/` on Linux), named after the file, the cell and the versions.

| Option | Effect |
|---|---|
| `-f, --format` | `render` only: `png` (default) or `svg` |
| `-o, --output FILE` | Where to write the image |
| `--size WxH` | Size in pixels, each side between 64 and 16384 (default `1600x1000`) |
| `--theme light|dark` | Background (default `light`) |
| `--cell CELL` | Layout cell to draw (default: the top cell) |
| `--expr EXPR` | Computed waveform signals to draw (`.raw` only); added to those in `.riku.toml` |

Waveform images show the signals that changed most (or the first ones, for a single version) and the expressions that produce a curve.

> [!NOTE]
> With `-f png|svg`, `diff` and `show` exit with 0 when the image is written, even with `--ci`. Use `-f json` to get the change status.

## `riku lvs`

```bash
riku lvs [REV] [--suggest | --update] [--log [-n N]] [--netgen]
         [--sch FILE --layout FILE] [--cell CELL] [-f text|json] [--ci] [-r REPO]
```

Checks a layout against its schematic in one version. By default it uses **manual links**: a file `lvs/<cell>.toml`, versioned with the design, that says which layout transistors (fingers) implement each schematic transistor. From those links Riku checks W, L and model, net correspondence, shorts, opens and pins, with no external tool. `--netgen` uses Netgen instead. The workflow, the links file and the Netgen option are explained in [lvs.md](lvs.md).

```text
$ riku lvs
Manual LVS  xschem/ota-5t.sch ↔ layout/ota-5t.gds (ota-5t) · worktree
  Linked: 9 of 9 schematic transistors · 24 of 24 in the layout
  different parameter in M1: W 4 ≠ 2
  different parameter in M2: W 4 ≠ 2
  different parameter in M3: W 18 ≠ 19
  different parameter in M4: W 18 ≠ 19
  different parameter in M8: W 20 ≠ 19
  Manual LVS: there are pending items.
```

| Option | Effect |
|---|---|
| `REV` | Version to check (commit, branch, tag); without it, the files on disk. If that version has no links file, the links on disk are used |
| `--suggest` | Add to `lvs/<cell>.toml` the links that can be deduced without guessing (creates the file). Working tree only |
| `--update` | Rewrite `lvs/<cell>.toml` with the current positions, after moving the layout. Working tree only |
| `--log` | The LVS in each of the last commits (first-parent history from `REV`, default `HEAD`), marking where it changed state |
| `-n, --limit N` | With `--log`: how many commits (default 20) |
| `--netgen` | Compare with Netgen instead of the links (needs `netgen`). Not combinable with `--suggest` or `--update` |
| `--sch FILE`, `--layout FILE` | The pair to check (both are required together) |
| `--cell CELL` | Layout cell to compare (default: the top cell) |
| `-f json` | JSON output (always indented) |
| `--ci` | CI exit codes (below) |

**Which pairs.** `--sch` and `--layout` if given; otherwise the `[[lvs]]` entries in `.riku.toml`; otherwise every schematic with a layout of the same name (`amp.sch` with `amp.gds`, `amp.oas` or `amp.mag`). When more than one layout matches, Riku picks one and prints a warning; pin it with `[[lvs]]`.

**Verdicts.** `clean` (everything checked and matching), `clean in transistors and pins` (some elements are not checked, such as resistors or sub-circuits without a schematic; they are listed as not checked) or `there are pending items`.

**Exit codes.** Without `--ci`: 0 when the check ran (whatever the verdict), 1 on error. With `--ci`:

| Mode | 0 | 1 | 2 |
|---|---|---|---|
| `riku lvs` (manual links) | Clean, everything checked | Pending items, differences, or partly checked | Error |
| `riku lvs --netgen` | Match | Parameters differ, or no match | Error |
| `riku lvs --log` | Newest commit clean | Newest commit not clean, or it could not be checked | Error before the history is built (no pair found, unknown revision) |
| `riku lvs --netgen --log` | Newest commit matches (or the pair is missing there) | Newest commit does not match | Newest commit errored |

JSON schemas: `riku-lvs-check/v1` (manual check), `riku-lvs-map-log/v1` (manual `--log`), `riku-lvs/v1` (`--netgen`) and `riku-lvs-log/v1` (`--netgen --log`). See [scripting.md](scripting.md#lvs-schemas).

## `riku open` and `riku gui`

```bash
riku open [FILE]
riku gui [FILE] [--cell CELL]
```

Both open the desktop viewer (`.sch`, `.gds`, `.oas`, `.mag`, `.raw`, `.sym`). The argument can also be a folder, which becomes the project folder. Without an argument, the project folder is the current one.

- **`riku open`** starts the viewer as a separate process and returns at once, so the terminal or the shell stays free. `diff -f visual` and `show -f visual` launch it the same way.
- **`riku gui`** runs the viewer in the current process and blocks until you close the window. Besides the file it accepts `--cell CELL` (the layout cell to open first), plus `--repo`, `--commit-a`, `--commit-b` and `--expr`, which `-f visual` uses internally to open a diff.

On Linux the viewer needs a graphical session (`DISPLAY` or `WAYLAND_DISPLAY`); without one, both commands fail with a message and the rest of the CLI keeps working. A `riku` built without the `gui` feature reports that too. The viewer itself is described in [viewer.md](viewer.md).

## `riku doctor`

```bash
riku doctor [-f text|json] [-r REPO]
```

Checks what Riku needs and lists the formats this build can compare:

```text
$ riku doctor

Riku Doctor — environment check

--- Git repository ---
  [ok]  ~/riku-demos/ota/
  [--]  .riku.toml: none (default options)

--- PDK ---
  [--]  .xschemrc: not found
  [ok]  $PDK_ROOT/$PDK → …/pdks/gf180mcuD/libs.tech/xschem
  [ok]  $TOOLS → …/tools/xschem/share/xschem/xschem_library/devices
  [ok]  Magic: .mag libraries in gf180mcuD (5), sky130A (6)

--- Format modules ---
  [ok]  xschem     Native renderer | PDK: gf180mcuD [ok]
  [ok]  layout     riku-mod-layout (gdstk cxx; Magic in Rust)
  [ok]  spice      waveforms (ngspice .raw)

--- LVS ---
  [ok]  netgen     …/tools/bin/netgen

Environment ready.
```

| Section | What it reports |
|---|---|
| Git repository | The repository root, and whether `.riku.toml` exists and parses (a parse error is shown here) |
| PDK | `.xschemrc`, `$PDK_ROOT`/`$PDK` (or the installed PDKs Riku will pick by symbols when `$PDK` is unset), `$TOOLS`, and the Magic `.mag` libraries of the installed PDKs. A warning if there is no symbol source at all |
| Format modules | Each module and whether it is available |
| LVS | Where `netgen` is; it is optional |

Marks: `[ok]` found, `[!]` set but wrong, `[--]` not set (not an error), `[x]` module not available. The symbol and PDK search is explained in [configuration.md](configuration.md#pdk-and-symbol-discovery). `-f json` (`riku-doctor/v1`) is described in [scripting.md](scripting.md#riku-doctorv1).

## `riku demo`

```bash
riku demo [NAME] [--dir DIR] [--list]
```

Creates example projects, each a Git repository with a history of real design changes, so you can try Riku without your own design.

```text
$ riku demo --list
  ota        5-transistor OTA (SKY130): schematic, layout and simulation; 11 commits, a branch and a short
  sram       16×8 OpenRAM SRAM (SKY130): sub-cell changes, a rename and fill; 7 commits and a branch
  inversor   5 V inverter in Magic and Xschem (SKY130): layers by name, a transistor in its sub-cell, ports, an open and the LVS; 11 commits and a branch
  chip       1 KB OpenRAM SRAM (SKY130, 9.9 MB GDS, 8,192 bitcells): a bitcell change, straps, a rename and pins; to see the performance with a large layout
```

- Without `NAME`, all of them are created; with a name, only that one.
- Each project goes to `<DIR>/<name>`. The default `DIR` is `~/riku-demos`.
- A project folder that already exists is left untouched (`already exists: left untouched`).
- The projects are embedded in the `riku` binary; creating them needs `git` but no network. They come with their branches and tags and no remote.
- Each project has a `README.md` with what to try. A good start is `cd ~/riku-demos/ota && riku log --graph`.

## `riku completions`

Prints a completion script for `bash`, `zsh`, `fish`, `powershell` or `elvish`:

```bash
riku completions bash > ~/.local/share/bash-completion/completions/riku
riku completions zsh  > "${fpath[1]}/_riku"
riku completions fish > ~/.config/fish/completions/riku.fish
```

## Exit codes

| Command | 0 | 1 | 2 |
|---|---|---|---|
| `status` | No changes, or cosmetic only | Functional changes | Error, or a file that could not be compared |
| `status --lvs` | LVS same or better | LVS got worse | LVS error |
| `diff`, `show` with `--ci` | No changes, or cosmetic only | Functional changes | Error, or a file that could not be compared |
| `diff`, `show` without `--ci` | Success, whatever changed | Error, or a file that could not be compared | — |
| `lvs --ci` | See [`riku lvs`](#riku-lvs) | | |
| Every other command | Success | Error | — |

A file that could not be compared (a damaged layout, a file over 50 MB) does not stop the command: the other files are still printed, and the exit code reports the failure. With `-f json`, errors are printed as JSON on stdout. Details and CI recipes are in [scripting.md](scripting.md).
