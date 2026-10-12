# Getting started

This guide installs Riku, walks through the bundled demo projects, and then points Riku at a design of your
own. It takes about fifteen minutes.

**On this page:** [Install](#install) · [In iic-osic-tools (Docker)](#in-iic-osic-tools-docker) ·
[Check your environment](#check-your-environment) · [Create the demos](#create-the-demos) ·
[Tour 1: an analog block](#tour-1-an-analog-block-ota) · [Tour 2: a Magic layout](#tour-2-a-magic-layout-inversor) ·
[Tour 3: a hierarchical GDS](#tour-3-a-hierarchical-gds-sram) · [Tour 4: LVS in five minutes](#tour-4-lvs-in-five-minutes-ota) ·
[Your own project](#your-own-project) · [Next steps](#next-steps)

## Install

Riku is a single executable for Linux x86_64 (glibc 2.35 or newer):

```bash
curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh
riku --version
```

The script downloads the latest release from GitHub, verifies its checksum and installs `riku` into
`~/.local/bin`, plus a **Riku** entry in your applications menu. If `~/.local/bin` is not on your `PATH`,
the installer tells you how to add it. To install system-wide, run it with `sudo sh -s -- latest --system`.
To remove it, run `install.sh --uninstall` from the release archive (add `--system` if you installed it there).

Optional, for Tab completion in your shell:

```bash
riku completions bash > ~/.local/share/bash-completion/completions/riku
```

> [!NOTE]
> Riku does not need Xschem, KLayout or Magic. For **GDS and OASIS** layouts in SKY130, GF180MCU and
> IHP SG13G2, transistors and nets work without any PDK installed: the rules are built into Riku.
> An installed PDK (`$PDK_ROOT`, or `/foss/pdks` in iic-osic-tools) is needed for:
> - transistors and nets in **Magic** (`.mag`) layouts, which are read with the PDK's Magic `.tech`
>   (without it, a `.mag` diff still shows the geometry; see [Formats](formats.md#where-the-rules-come-from));
> - the PDK cells a `.mag` uses (for example `sky130_fd_sc_hd__inv_1`): without them they are compared as empty, with a warning;
> - drawing schematic symbols and running the LVS.
>
> See [Configuration](configuration.md).

## In iic-osic-tools (Docker)

Riku runs inside the [iic-osic-tools](https://github.com/iic-jku/IIC-OSIC-TOOLS) container like any other tool.
Pick the PDK with `sak-pdk` first; it sets `PDK_ROOT`, `PDK` and `PDKPATH`, which Riku reads:

```bash
sak-pdk sky130A
riku doctor              # the PDK and netgen should show as [ok]
```

- Every command works without a display. The viewer (`riku open`, `riku gui`, `-f visual`) needs the
  container's desktop (VNC or X11 forwarding), with `DISPLAY` set.
- For images without a desktop, use `riku render` or `-f png` (see [Images](cli.md#images-riku-render-and--f-pngsvg)).
- When the container runs on Windows, the viewer's folder picker also accepts Windows paths (see [Viewer](viewer.md#opening-a-folder)).

Riku speaks Spanish too: `export RIKU_LANG=es` for the command line, **Settings → Language** in the viewer.

## Check your environment

`riku doctor` reports what Riku sees: the Git repository, the PDK, which format modules are available, and
whether the optional LVS tool (Netgen) is installed.

```text
$ riku doctor

Riku Doctor — environment check

--- Git repository ---
  [ok]  ~/riku-demos/ota/
  [--]  .riku.toml: none (default options)

--- PDK ---
  [--]  .xschemrc: not found
  [ok]  $PDK_ROOT/$PDK → …/pdks/gf180mcuD/libs.tech/xschem
  [ok]  $TOOLS → …/xschem/share/xschem/xschem_library/devices
  [ok]  Magic: .mag libraries in gf180mcuD (5), sky130A (6)

--- Format modules ---
  [ok]  xschem     Native renderer | PDK: gf180mcuD [ok]
  [ok]  layout     riku-mod-layout (gdstk cxx; Magic in Rust)
  [ok]  spice      waveforms (ngspice .raw)

--- LVS ---
  [ok]  netgen     …/bin/netgen

Environment ready.
```

`[--]` lines are informational; Riku runs without them.

## Create the demos

```bash
riku demo            # creates all demos in ~/riku-demos
riku demo --list     # what each one contains
riku demo ota --dir ~/tmp   # one demo, somewhere else (creates ~/tmp/ota)
```

Each demo is an ordinary Git repository with a scripted history, and a `README.md` listing things to try.
They use real open-source designs (credited in each `README.md`), changed on purpose to show Riku's diffs.

## Tour 1: an analog block (`ota`)

```bash
cd ~/riku-demos/ota
riku log --graph
```

```text
● 421f72d (HEAD, main, v1.0)  Rename the tail node: node -> tail
│   Riku Demo · 2026-03-08 08:30
│   xschem/ota-5t.sch  3 components modified, 1 net added, 1 net removed
● d4e936d  Testbench: 2 pF load
│   Riku Demo · 2026-03-07 19:30
│   sim/ota-5t_tb.raw  38 signals changed
│   xschem/ota-5t_tb.sch  1 component modified
● 6184836  Fix: the Vout route no longer touches Vp
│   Riku Demo · 2026-03-07 06:30
│   layout/ota-5t.gds  1 component removed, 1 net modified
● 120ee0b  Layout: route Vout to the left edge
│   Riku Demo · 2026-03-06 17:30
│   layout/ota-5t.gds  1 short, 1 component added
○   ad104a7 [merge]  Merge branch 'narrow-input-pair'
│     Riku Demo · 2026-03-06 04:30
├─╮
● │ 6568371  Tidy up the schematic (move everything)
│ │   Riku Demo · 2026-03-05 15:30
│ │   xschem/ota-5t.sch  (cosmetic changes only)
…
```

Every commit gets a one-line summary per file. The schematic that was only rearranged is marked
**cosmetic**; the layout commit that shorted two nets says so.

### What Riku sees in a schematic

The last commit renamed a node. A text diff of the `.sch` shows edited coordinates and attributes; Riku
shows which labels changed and which net appeared and which went away:

```text
$ riku show HEAD
…
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

Riku follows the hierarchy within the same version. Between the two tags, the testbench's load changed,
and so did the OTA inside its instance `x1`:

```text
$ riku diff v0.1 v1.0 xschem/ota-5t_tb.sch
File     : xschem/ota-5t_tb.sch
Changes  : 2

  ~ C1
      value: 1p → 2p
  ~ x1
      changed inside: xschem/ota-5t.sch
```

A commit that only moves things around (`riku show 6568371`, "Tidy up the schematic") reports
`no semantic changes`. Riku compares components, parameters and nets by name, not pin-by-pin wiring;
see [Formats → Xschem](formats.md#xschem-schematics-sch).

To look at a change without opening a window, ask for an image; Riku prints where it wrote it:

```bash
riku diff v0.1 v1.0 xschem/ota-5t.sch -f png
```

### A schematic change and its effect on the simulation

```bash
riku show HEAD~7
```

The commit widened two PMOS transistors and re-ran the simulation. Riku shows both: the parameter change
in the schematic, and every signal of the `.raw` file that moved more than the tolerance.

```text
File     : sim/ota-5t_tb.raw
Changes  : 65
Cosmetic : 61

  ~ v(vout)
      Δmax 44.826 mV at 1.800 V · RMS 44.826 mV · 6.88 % of range  (Operating Point)
  …

File     : xschem/ota-5t.sch
Changes  : 2

  ~ M1
      W: 2 → 4
  ~ M2
      W: 2 → 4
```

### A short in the layout, and its fix

```bash
riku show HEAD~3
riku show HEAD~2
```

```text
  ! ota-5t:net:Vout = Vp
      short (nets joined): Vout, Vp → Vout = Vp
      bbox: (-5.600, -0.560) → (-0.400, 9.550) µm
```

```text
  ~ ota-5t:net:Vout = Vp
      nets separated (short fixed): Vout = Vp → Vout, Vp
```

Riku extracted the nets of both versions with the SKY130 rules and saw that two labeled nets became one.
Changes that alter connectivity are marked with `!`, listed first, and carry `"severity": "error"` in JSON.

### Transistor sizes on a branch

```bash
riku show narrow-input-pair
```

```text
  ~ ota-5t:sky130_fd_pr__nfet_01v8 @ (0.150, 3.367)
      w_um: 5.000 → 4.500
```

A layout edit that trimmed the diffusion is reported as what it means electrically: six fingers went
from W = 5 µm to 4.5 µm.

### Comparing any two versions

```bash
riku diff v0.1 v1.0                                     # everything between two tags
riku diff v0.1 v1.0 sim/ota-5t_tb.raw --expr "ac: a0 = max(db(v(vout)))"
riku diff v0.1 v1.0 layout/ota-5t.gds -f visual         # opens the viewer
```

```text
  ~ a0
      = max(db(v(vout)))
      38.292 dB → 38.881 dB · Δ 0.589 dB (1.51 %)  (AC Analysis)
```

`--expr` evaluates ngspice-style expressions on both versions; see [Formats → Expressions](formats.md#expressions).

### The viewer

```bash
riku open
```

The home screen shows the project, uncommitted changes and recent files. Press **H** for the commit graph,
click a commit and then a file to see its diff. In a layout diff, the **Diff**, **Before** and **After**
views keep the same zoom, and clicking a change in **Details → Changes** zooms to it. See [Viewer](viewer.md).

## Tour 2: a Magic layout (`inversor`)

```bash
cd ~/riku-demos/inversor
riku log --graph
```

Magic stores one cell per file with named layers. Riku compares in those layers and follows sub-cells
from the same commit.

A port class change:

```text
$ riku show 721c1e3
File     : layout/inv.mag
Changes  : 2

  ~ inv:port:in
      class: — → input
      use: — → signal
  ~ inv:port:out
      class: — → output
      use: — → signal
```

A change made inside a sub-cell file, seen from the cell that uses it:

```text
$ riku show e08504b
File     : layout/inv.mag
Changes  : 19

  ~ inv:polycont:sky130_fd_pr__nfet_g5v0d10v5_H9JWFY
      origin: inv → sky130_fd_pr__nfet_g5v0d10v5_H9JWFY @ (1.350, -2.865)
      +1 polys / +0.058 µm²
      -1 polys / -0.058 µm²
  …
```

And a re-save in Magic that only touched timestamps, which a text diff would flag on every file:

```text
$ riku show 3a1ee46
File     : layout/inv.mag
  no semantic changes
…
```

## Tour 3: a hierarchical GDS (`sram`)

```bash
cd ~/riku-demos/sram
riku show 4437d9e        # Bitcell: 20 nm wider bitline metal2
```

One edit to the bitcell's metal is reported once in the cell that changed, and once in each parent with
how many instances inherit it:

```text
  + sky130_fd_bd_sram__sram_sp_cell_met2:L69/20
      +3 polys / +0.086 µm²
  …
      origin: sram_16x8_sky130_sky130_bitcell_array → sky130_fd_bd_sram__sram_sp_cell_opt1 (in 79 instances)
```

A renamed cell with identical geometry is a rename, not a delete plus an add:

```text
$ riku show c6cf2ee
  r cell:sram_16x8_sky130_pmos_m1_w1_120_sli_dli_da_p → sram_16x8_sky130_pmos_w1p12_da
```

For a large layout, try the `chip` demo (a 9.9 MB GDS with 8,192 bitcells). The first comparison of a
commit takes a few seconds; Riku caches results, so repeating it is much faster.

## Tour 4: LVS in five minutes (`ota`)

`riku lvs` checks the layout against the schematic with a links file, `lvs/<cell>.toml`, that says which
schematic transistor is which in the layout. The demo has none yet; `--suggest` writes the links that
follow without guessing:

```text
$ cd ~/riku-demos/ota
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

Every transistor is linked, and the check finds what the demo's history left behind: the schematic
widened M1 and M2 to 4 µm, and the layout still has 2 µm. Fix the layout or the schematic, run `riku lvs`
again, and commit `lvs/ota-5t.toml` with the design so the links travel with it. The viewer shows the same
links side by side (**LVS** in the top bar), where you link what `--suggest` could not.

> [!NOTE]
> `riku log --lvs` and `riku status --lvs` use Netgen, not your links file. See [LVS](lvs.md#lvs-in-the-history-log---lvs-and-status---lvs).

The full workflow (moving the layout, history, CI) is in [LVS](lvs.md).

## Your own project

Riku works in any Git repository that contains `.sch`, `.gds`, `.oas`, `.mag` or `.raw` files. There is
nothing to initialize:

```bash
cd path/to/your/design
riku status                  # what changed on disk, functional or cosmetic
riku diff                    # the details, working tree vs HEAD
riku diff path/to/amp.sch    # one file
riku log -n 20 --graph       # recent history
```

Paths are relative to where you are, like in Git. A few things are worth setting up once:

- **The PDK.** Export `PDK_ROOT` and `PDK` (for example `PDK=sky130A`) so Riku can draw schematic symbols,
  build the schematic netlist for the LVS, and read Magic layouts (their transistors, nets and PDK cells).
  GDS/OASIS transistors and nets do not need it. Riku also reads `.xschemrc`. See [Configuration](configuration.md).
- **Project options.** A `.riku.toml` at the repository root fixes options such as the waveform
  tolerance or expressions to always compare, so everyone (and CI) compares the same way.
- **LVS.** If your project has a schematic and a layout of the same cell, run `riku lvs --suggest` to start
  checking one against the other. See [LVS](lvs.md).

> [!TIP]
> In scripts and CI, use `-f json` and the exit codes: `riku status` exits with `1` when there are
> functional changes. See [Scripting and CI](scripting.md).

## Next steps

- [Troubleshooting](troubleshooting.md): when something does not work as expected.
- [Command reference](cli.md): every command and option.
- [Formats](formats.md): exactly what Riku compares in each file type, and the limits.
- [Viewer](viewer.md): navigation, shortcuts, waveforms and the LVS view.
- [LVS](lvs.md): checking a layout against its schematic, at any commit.
