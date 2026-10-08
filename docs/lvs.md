# Layout versus schematic (LVS)

`riku lvs` checks that a layout implements its schematic, at any version of your repository, from links between schematic and layout transistors that you keep in a versioned file next to the design.

On this page:

- [How it works](#how-it-works)
- [What Riku checks](#what-riku-checks)
- [Verdicts](#verdicts)
- [Workflow](#workflow)
- [The links file](#the-links-file)
- [Which schematic goes with which layout](#which-schematic-goes-with-which-layout)
- [Caching](#caching)
- [Netgen mode](#netgen-mode)
- [LVS in the history: `log --lvs` and `status --lvs`](#lvs-in-the-history-log---lvs-and-status---lvs)
- [Exit codes](#exit-codes)
- [JSON output](#json-output)
- [Limits](#limits)

## How it works

Riku's LVS is **manual and progressive**. You (or `riku lvs --suggest`) say which schematic transistor corresponds to which layout transistors (its fingers). The links live in `lvs/<cell>.toml`, committed with the design. From those links Riku derives everything else: whether the parameters agree, which layout net is each schematic net, and whether two links contradict each other (a short or an open).

- Riku writes the schematic netlist itself and extracts transistors and nets from the layout itself. You do not need Xschem, Magic or Netgen. You do need the design's PDK installed (`$PDK_ROOT` and `$PDK`, or a PDK Riku can detect from the schematic's symbols), because the netlist uses its symbols; `riku doctor` checks this.
- A layout transistor is identified by its model and the center of its gate, in µm and in the coordinates of the compared cell. Renumbering devices, moving the cell inside a chip or adding unrelated geometry does not break a link.
- Because the links are versioned, `riku lvs` works on any commit, and `riku lvs --log` shows where the state changed.

Netgen is available as an alternative that compares everything at once without links: see [Netgen mode](#netgen-mode).

## What Riku checks

For each linked pair (one schematic transistor and its layout fingers):

| Check | Rule |
|---|---|
| **W** | The schematic W (times its multiplier `m`) must equal the **sum** of the W of its layout fingers, within 0.1 % (at least 0.005 µm). Reported as `different parameter in M1: W 4 ≠ 2` |
| **L** | Every finger's L must equal the schematic L, with the same tolerance |
| **Model** | The layout model must be the schematic model, with or without the library prefix (`sky130_fd_pr__nfet_01v8` = `nfet_01v8`). Reported as `different model in …` |

Across all links:

| Check | Rule |
|---|---|
| **Net correspondence** | From the terminals of the linked transistors (gate, body, and source/drain in whichever order fits), Riku works out which layout net is each schematic net |
| **Shorts** | One layout net that several schematic nets map to: `SHORT: layout net … joins …` |
| **Opens** | One schematic net split across several layout nets: `OPEN: … is split in the layout: …` |
| **Pins** | Every schematic pin must exist in the layout and reach the same net the links say it should; a layout pin the schematic does not have is also reported. Pin names are compared without case |
| **Coverage** | How many schematic transistors and layout fingers are linked: `Linked: 9 of 9 schematic transistors · 24 of 24 in the layout` |

Shorts and opens appear as soon as two links contradict each other, so you see them while you are still linking, not only at the end.

Riku also reports links that no longer resolve: **lost** fingers (nothing of that model at that position, even after looking for a move, see [After moving the layout](#5-after-moving-the-layout)) and links to a transistor the schematic no longer has.

## Verdicts

Each pair ends with one of three verdicts:

| Verdict | Meaning |
|---|---|
| `Manual LVS: clean.` | Every transistor on both sides is linked, with no different parameters or models, no shorts, opens, lost links or pin problems, and nothing left unchecked |
| `Manual LVS: clean in transistors and pins (some elements are not checked).` | Everything above holds, but the design has elements Riku does not check (resistors, capacitors, sub-circuits without a schematic), listed as `not checked: …` |
| `Manual LVS: there are pending items.` | Something is unlinked, different, shorted, open, lost, or a pin does not match |

With `--ci`, only the first verdict exits with 0 (see [Exit codes](#exit-codes)).

## Workflow

The examples use the `ota` demo (`riku demo ota`, then `cd ~/riku-demos/ota`), a 5-transistor OTA in SKY130 whose layout drifted from the schematic over its history.

### 1. See where you stand

Without a links file, `riku lvs` lists every transistor on both sides as not linked:

```text
$ riku lvs
Manual LVS  xschem/ota-5t.sch ↔ layout/ota-5t.gds (ota-5t) · worktree
  lvs/ota-5t.toml does not exist yet: riku lvs --suggest starts it
  Linked: 0 of 9 schematic transistors · 0 of 24 in the layout
  not linked in the schematic: M4, M1, M2, M3, M5, M6, M7, M8, M9
  not linked in the layout: nfet_01v8 (-0.950, -15.420), nfet_01v8 (0.550, -15.420), nfet_01v8 (2.050, -15.420), …
  Manual LVS: there are pending items.
```

### 2. Let Riku suggest the obvious links

```text
$ riku lvs --suggest
Manual LVS  xschem/ota-5t.sch ↔ layout/ota-5t.gds (ota-5t) · worktree
  9 links suggested: M4, M3, M6, M7, M8, M9, M1, M2, M5
  → lvs/ota-5t.toml
  Linked: 9 of 9 schematic transistors · 24 of 24 in the layout
  different parameter in M4: W 18 ≠ 19
  different parameter in M3: W 18 ≠ 19
  different parameter in M8: W 20 ≠ 19
  different parameter in M1: W 4 ≠ 2
  different parameter in M2: W 4 ≠ 2
  Manual LVS: there are pending items.
```

Suggestions never guess. They start from the nets that have the same name on both sides (the pins and other labels), then link a schematic transistor only when exactly one group of parallel layout fingers (same model, L and terminals) fits the nets already linked. Each new link adds known nets, and the process repeats until nothing else follows. Two transistors that could swap places (a differential pair with nothing that tells them apart) are left for you to link by hand. A layout without labels gives the suggestions nothing to start from.

`--suggest` only adds links; it keeps the ones already in the file. You can run it again at any time.

### 3. Review and link the rest in the viewer

Open the schematic or the layout in the viewer and click **LVS**:

```bash
riku gui xschem/ota-5t.sch      # then click LVS in the top bar
```

The **Links** tab shows each transistor in green (linked), orange (linked with differences) or grey (not linked) on both canvases. Click a schematic transistor and its fingers in the layout (**Shift + click** adds fingers), then **Link**. Each change is written to `lvs/<cell>.toml` immediately. See [viewer.md](viewer.md#lvs-view) for all the controls.

Review the suggested links too: in this demo they are all correct, and the five orange transistors are real differences. The schematic widened M1 and M2 to 4 µm and narrowed M3 and M4 to 18 µm, and the layout never followed.

### 4. Check

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

Commit `lvs/ota-5t.toml` with the design. From then on, the links travel with the history.

### 5. After moving the layout

Moving the cell inside a larger layout changes nothing (positions are in the cell's own coordinates), and neither does moving an instance of a sub-cell that contains transistors: each finger drawn in a sub-cell is also recorded by its position inside that sub-cell. Riku relinks those by their sub-cell and says so: `relinked by their sub-cell (an instance moved): …`.

If everything inside the cell moved, rotated or was mirrored, Riku finds the rigid move that realigns the links, preferring the one that contradicts the nets and their names the least. It reports the move as `the cell moved: <angle>° and (<dx>, <dy>) µm; <N> relinked (riku lvs --update saves it)`, with `mirrored` after the angle when it was mirrored.

If several moves fit equally well, Riku relinks nothing and says so (`the cell seems to have moved, but it can be aligned in more than one way: nothing was relinked (link them again)`): it prefers a lost link to a wrong one. A single transistor that moved on its own is relinked by connectivity when it is the only candidate of the right model connected to the nets already linked (`relinked by connectivity: …`).

Relocated links are checked normally, but the file still has the old positions. Save the new ones:

```bash
riku lvs --update
```

`--update` rewrites `lvs/<cell>.toml` with the current position of every finger it found. In the viewer, **Save positions** does the same.

### 6. Look back in history

```bash
riku lvs HEAD~3          # the state at a commit
riku lvs --log -n 12     # commit by commit, from HEAD (or from REV: riku lvs --log v1.0)
```

```text
$ riku lvs --log -n 12
Manual LVS  xschem/ota-5t.sch ↔ layout/ota-5t.gds  (last 9 commits from HEAD)
  421f72d  2026-03-08 08:30  Rename the tail node: node -> tail        9/9 linked · 5 differences · 0 shorts · 0 opens
  d4e936d  2026-03-07 19:30  Testbench: 2 pF load                      9/9 linked · 5 differences · 0 shorts · 0 opens
  6184836  2026-03-07 06:30  Fix: the Vout route no longer touches Vp  9/9 linked · 5 differences · 0 shorts · 0 opens  ← a short is gone
  120ee0b  2026-03-06 17:30  Layout: route Vout to the left edge       9/9 linked · 5 differences · 1 shorts · 0 opens  ← a short appeared
  ad104a7  2026-03-06 04:30  Merge branch 'narrow-input-pair'          9/9 linked · 5 differences · 0 shorts · 0 opens
  6568371  2026-03-05 15:30  Tidy up the schematic (move everything)   9/9 linked · 2 differences · 0 shorts · 0 opens
  e64f5f2  2026-03-05 02:30  Layout: metal5 power straps on VDD and V  9/9 linked · 2 differences · 0 shorts · 0 opens
  a603147  2026-03-03 11:30  Wider PMOS load: M1, M2 W 2u -> 4u        9/9 linked · 2 differences · 0 shorts · 0 opens  ← no longer clean
  5338fc4  2026-03-02 22:30  Initial OTA: schematic, testbench, layou  clean · 9/9 linked
```

`--log` follows the first parent (merged branches are not walked; `-n` defaults to 20). The marks show where the state changed: `clean`, `no longer clean`, `a short appeared`, `a short is gone`, `an open appeared`, `an open is gone`, and `+N linked`.

**Commits without a links file use the links on disk.** In this demo the file was never committed, yet every commit above is checked with today's links. This is how you check a history that predates the links: link once, then look back. For a single commit, Riku says `this version has no lvs/ota-5t.toml: the links on disk are used`.

`--suggest` and `--update` write the file, so they only work on the working tree, not with a `REV` or `--log`.

### 7. Gate CI on it

```text
$ riku lvs --ci
Manual LVS  xschem/ota-5t.sch ↔ layout/ota-5t.gds (ota-5t) · worktree
  …
  Manual LVS: there are pending items.

[exit 1]
```

With `--ci`, `riku lvs` exits with 0 only when every pair is clean with nothing left unchecked. See [Exit codes](#exit-codes) and [scripting.md](scripting.md) for CI recipes.

## The links file

`riku lvs --suggest` (and every change in the viewer) writes `lvs/<cell>.toml` at the repository root, where `<cell>` is the compared layout cell. This is the file it wrote for the `ota` demo, trimmed:

```toml
# Qué transistor del esquemático es cuál del layout (riku: LVS manual).
# Cada transistor del layout: su modelo y un punto de su compuerta, en µm de la celda.
schema = "riku-lvs-map/v1"
schematic = "xschem/ota-5t.sch"
layout = "layout/ota-5t.gds"

[[bind]]
schematic = "M1"
layout = [
  { model = "sky130_fd_pr__pfet_01v8", at = [0.600, 13.100] },
  { model = "sky130_fd_pr__pfet_01v8", at = [5.100, 13.100] },
]

[[bind]]
schematic = "M3"
layout = [
  { model = "sky130_fd_pr__nfet_01v8", at = [1.150, -6.100] },
  { model = "sky130_fd_pr__nfet_01v8", at = [2.150, -6.100] },
  { model = "sky130_fd_pr__nfet_01v8", at = [3.150, 3.350] },
  { model = "sky130_fd_pr__nfet_01v8", at = [4.150, 3.350] },
]

# … one [[bind]] per schematic transistor
```

Riku starts the file with a two-line comment. In version 0.2.0 that comment is always written in Spanish; it says "Which schematic transistor is which in the layout (riku: manual LVS). Each layout transistor: its model and a point of its gate, in µm of the cell."

| Field | Meaning |
|---|---|
| `schema` | File format version; must be `riku-lvs-map/v1`. Any other value is an error (`unknown file version`) |
| `schematic`, `layout` | The pair, relative to the repository root |
| `cell` | The compared layout cell, only when it is not the top cell (set by `--cell` or `[[lvs]]`) |
| `[[bind]]` | One link per schematic transistor |
| `bind.schematic` | The schematic instance: `M1`, or `x1/M3` for a transistor inside a sub-circuit instance `x1` |
| `bind.layout` | Its fingers. Each finger: `model`, and `at`, the center of its gate in µm in the compared cell's coordinates |
| `cell`, `local` (in a finger) | Only for a finger drawn inside a sub-cell: that sub-cell's name and the gate center in its own coordinates. Used to relink the finger when its instance moves |

A finger matches a layout transistor of the same model within 0.01 µm of `at`.

Riku rewrites the whole file whenever it saves: links sorted by name (`M2` before `M10`), fingers sorted by position, coordinates with three decimals, so diffs of the file stay small and readable. You can edit it by hand (add a finger, remove a link), but comments you add are lost the next time Riku writes it.

## Which schematic goes with which layout

From highest to lowest priority:

1. `--sch FILE --layout FILE` on the command line (both are required together; `--cell` picks the layout cell).
2. `[[lvs]]` entries in `.riku.toml`, one per pair:

   ```toml
   [[lvs]]
   schematic = "xschem/amp.sch"
   layout = "layout/amp.gds"
   cell = "amp"          # optional: without it, the top cell
   ```

3. Same name: every Xschem schematic with a layout of the same file name anywhere in the repository (`amp.sch` ↔ `amp.gds`). If several layouts share the name, Riku prefers `.gds`, then `.oas`, then `.mag`, and warns that you can fix the choice with `[[lvs]]`. Hidden folders and `target` are skipped. A schematic without a same-named layout (a testbench) is not a pair.

`riku lvs` checks every pair it finds. Without any pair it stops with `no schematic with a layout to compare: use --sch and --layout, [[lvs]] in .riku.toml, or give them the same name (amp.sch and amp.gds)`. See [configuration.md](configuration.md) for the rest of `.riku.toml`.

## Caching

Extracting both sides (the schematic netlist and the layout's transistors and nets) is the slow part; checking the links is immediate. Riku caches each extraction together with everything it read: every project file with its Git id (including files it looked for and did not find) and a fingerprint of the environment (Riku version, netlister, PDK). A cached extraction is reused only if none of that changed, both for a commit and for the working tree. When checking a commit, the cache is validated by reading from Git, without writing the commit to disk. Editing the links file never invalidates it.

| Cache | Location |
|---|---|
| Manual LVS extractions | `~/.cache/riku/lvs/manual-v1/` |
| Netgen results (`--netgen`, `log --lvs`, `status --lvs`) | `~/.cache/riku/lvs/v2/` |

The base folder is `$XDG_CACHE_HOME/riku` when that variable is set. `RIKU_CACHE_DIR=DIR` moves it to `DIR/lvs/…`, and `RIKU_NO_CACHE=1` disables the disk cache. Riku keeps the 200 newest entries per pair. Deleting the folders is always safe.

## Netgen mode

`riku lvs --netgen` compares the whole circuit with [Netgen](http://opencircuitdesign.com/netgen/) instead of the links: no links file needed. Riku writes both netlists (the schematic's with its own netlister, the layout's with its own extraction) and runs Netgen with the PDK's setup file.

```text
$ riku lvs --netgen
LVS  xschem/ota-5t.sch ↔ layout/ota-5t.gds (ota-5t) · worktree · sky130A
  The connections match, but some parameters differ.
  devices: 8 in the schematic, 8 in the layout · nets: 8 and 8
  Different parameters (5), schematic ≠ layout:
    M1 ↔ 19 (sky130_fd_pr__pfet_01v8): w 4 ≠ 2
    M2 ↔ 20 (sky130_fd_pr__pfet_01v8): w 4 ≠ 2
    M4 ↔ 9 (sky130_fd_pr__nfet_01v8): w 18 ≠ 19
    M3 ↔ 7 (sky130_fd_pr__nfet_01v8): w 18 ≠ 19
    M8 ↔ 0 (sky130_fd_pr__nfet_01v8): w 20 ≠ 19
  Netgen: Final result: Circuits match uniquely. Property errors were found.
```

**Requirements.** `netgen` on your `PATH`, and the PDK's Netgen setup at `$PDK_ROOT/<pdk>/libs.tech/netgen/<pdk>_setup.tcl` (the PDK is the one whose symbols the schematic uses). Without them Riku says what is missing. `riku doctor` reports whether Netgen is installed. Environments such as iic-osic-tools ship Netgen and the PDKs; Riku does not need Netgen for anything else.

**What it reports.** One of three verdicts (`Match: same devices, connections and parameters.`, `The connections match, but some parameters differ.`, `They do NOT match.`), the device and net counts on each side, each parameter that differs, nets and devices without a pair, the pins when their counts differ, and Netgen's final line. Layout devices are named by their index in Riku's extraction (`19`). Netgen merges transistors in parallel, even with different L, so its counts can be lower than the transistor counts of the manual LVS. When the connections do not match (a short or an open), Netgen does not compare parameters.

To inspect the netlists and Netgen's full report, set `RIKU_LVS_KEEP=DIR`: Riku copies `schematic.spice`, `layout.spice`, `comp.out` and `comp.json` there.

**History.** `riku lvs --netgen --log [-n N] [REV]` runs Netgen at each commit (first parent) and marks where the verdict changed: `← stopped matching`, `← got worse: the connections no longer match`, `← got better: the connections match again`, `← matches again`, with the discrepancies that appeared, changed or were fixed below each commit. Netgen runs only for commits where something the comparison reads changed; the rest come from the cache.

In the viewer, the **Netgen** tab of the LVS view runs the same comparison on demand ([viewer.md](viewer.md#netgen-tab)).

## LVS in the history: `log --lvs` and `status --lvs`

> [!NOTE]
> `riku log --lvs` and `riku status --lvs` use **Netgen**, not the links file. They need Netgen and the PDK setup described in [Netgen mode](#netgen-mode).

**`riku log --lvs`** adds the LVS of every pair to each commit of the usual log, compared with the commit's first parent:

```text
$ riku log --lvs -n 12
LVS: 10 new Netgen run(s); the rest from the cache
…
* 120ee0b  Layout: route Vout to the left edge
          Riku Demo · 2026-03-06 17:30
          layout/ota-5t.gds  1 short, 1 component added
          LVS ota-5t: do NOT match  ← got worse: the connections no longer match
            + unmatched nets: Vout, Vp (layout: Vout)
            + pin Vp only in the schematic
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

- `+` a discrepancy appeared, `~` its value changed, `−` it was fixed. Discrepancies are matched across versions by their **schematic** names, because layout device numbers shift between versions. The percentage is the layout's error relative to the schematic.
- Without `--detail`, up to three lines per commit, and nothing for commits where the LVS did not change. `--detail` shows everything.
- It works with `--graph` too (`riku log --graph --lvs`).
- Without Netgen, `log` still prints, with a warning that the LVS is not available. `log --lvs` never changes the exit code of `log`.

**`riku status --lvs`** compares the LVS of the working tree with `HEAD`, and its exit code becomes the LVS one:

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

| Case | Exit code |
|---|---|
| The same or better than `HEAD` | 0 |
| The same verdict, but new discrepancies appeared | 0, with a warning on stderr |
| Some pair stopped matching or got worse | 1 |
| Error (Netgen missing, a netlist that cannot be built) | 2 |

A pair the changes did not touch prints on one line with `(unchanged)`.

### Pre-commit hook

To stop commits that break the LVS, add this to `.git/hooks/pre-commit` and make it executable (`chmod +x .git/hooks/pre-commit`):

```bash
#!/bin/sh
riku status --lvs > /dev/null
```

The hook compares the files on disk (not only what is staged) with `HEAD`, and blocks the commit with exit code 1 when a pair stopped matching or got worse, or 2 when the LVS could not run.

## Exit codes

| Command | 0 | 1 | 2 |
|---|---|---|---|
| `riku lvs` | Checked (whatever the verdict) | Error | — |
| `riku lvs --ci` | Every pair clean, nothing unchecked | Something pending or different | Error |
| `riku lvs --log --ci` | The newest commit clean, nothing unchecked | The newest commit pending, or it could not be checked | Error |
| `riku lvs --netgen --ci` | Match | Different parameters or no match | Error |
| `riku status --lvs` | See the table above | | |

Without `--ci`, `riku lvs` exits with 1 on any error. More on exit codes in [scripting.md](scripting.md).

## JSON output

`-f json` prints the same results with a versioned schema:

| Command | Schema |
|---|---|
| `riku lvs` | `riku-lvs-check/v1` |
| `riku lvs --log` | `riku-lvs-map-log/v1` |
| `riku lvs --netgen` | `riku-lvs/v1` |
| `riku lvs --netgen --log` | `riku-lvs-log/v1` |

A `riku-lvs-check/v1` result has, per pair, `clean` and `complete` (the two verdict conditions), `bound` (each link with its fingers as `model` and `at`), `params`, `models`, `shorts`, `opens`, `pins`, `lost`, `unknown`, `unbound_schematic`, `unbound_layout`, `unchecked`, `nets` (each schematic net and its layout nets), `moved`, `by_connectivity`, `by_cell`, `map` (the links file), `map_from_disk` and `cached`. Field details and examples are in [scripting.md](scripting.md).

## Limits

- **Only MOS transistors are checked.** Resistors, capacitors and sub-circuits whose definition is not in the netlist (devices without a schematic) are listed as `not checked: …`, and layout resistors as `N resistors of the layout`. A design with any of them reaches at most `clean in transistors and pins`. Sub-circuits from your own project are flattened, and their transistors are named by path (`x1/M3`).
- **Symmetric devices need manual linking.** `--suggest` does not choose between transistors that the nets cannot tell apart, such as the two halves of a differential pair before their outputs are linked.
- **Nets are seen through the linked transistors.** Shorts and opens are found from the terminals of linked transistors; a net that touches no linked transistor is only checked through the pins.
- **No Tcl in schematic code blocks.** Riku's netlister expands Xschem's `tcleval(…)` attributes with the variables of the PDK's `xschemrc`, but it has no Tcl interpreter: code blocks that are Tcl programs (loops, `xschem` commands) are not evaluated.
- **Netgen naming.** In Netgen mode, layout devices are numbered by the extraction, and transistors in parallel are merged, so names and counts differ from the manual LVS.

> [!NOTE]
> The `inversor` demo (`riku demo inversor`) does not include a links file, and the LVS output its README describes comes from Netgen: `riku log --graph --lvs` uses Netgen, and to see the commit where M9 stopped matching (`riku lvs HEAD~7` in the README) add `--netgen`. Plain `riku lvs` shows the manual LVS, with nothing linked until you run `riku lvs --suggest`.
