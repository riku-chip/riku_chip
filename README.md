<div align="center">

<img src="packaging/riku.svg" alt="Riku logo" width="96">

# Riku

**Semantic version control for chip design, on top of Git.**

Review changes to schematics, layouts and simulations at the level of the circuit, not the text.

[![CI](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml/badge.svg)](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/riku-chip/riku_chip)](https://github.com/riku-chip/riku_chip/releases)
[![Platform](https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey)](#installation)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

[Getting started](https://github.com/riku-chip/riku_chip/blob/main/docs/getting-started.md) ·
[Commands](https://github.com/riku-chip/riku_chip/blob/main/docs/cli.md) ·
[Viewer](https://github.com/riku-chip/riku_chip/blob/main/docs/viewer.md) ·
[Formats](https://github.com/riku-chip/riku_chip/blob/main/docs/formats.md) ·
[LVS](https://github.com/riku-chip/riku_chip/blob/main/docs/lvs.md)

</div>

---

`git diff` on an Xschem schematic shows moved coordinates. On a GDS it says `Binary files differ`.
Riku reads the versions stored in your Git history and tells you what actually changed in the circuit:

```text
$ riku show HEAD~3
commit 120ee0b  (parent ad104a7)

    Layout: route Vout to the left edge

File     : layout/ota-5t.gds
Changes  : 2

  ! ota-5t:net:Vout = Vp
      short (nets joined): Vout, Vp → Vout = Vp
      bbox: (-5.600, -0.560) → (-0.400, 9.550) µm
  + ota-5t:L70/20
      +1 polys / +6.716 µm²
```

A new piece of metal shorted two nets. Riku found it from the layout alone, and it marks it as an error
(`!`) in the terminal, in the viewer and in JSON for CI.

## What it understands

| Format | Files | What Riku reports |
|---|---|---|
| **Xschem** | `.sch` (`.sym` in the viewer) | Components added, removed, renamed or with new parameters; nets connected or disconnected; whether a change was only a visual rearrangement |
| **GDSII / OASIS** | `.gds`, `.oas` | Which area changed, on which layer and cell, and whether it comes from an instantiated sub-cell. With SKY130, GF180MCU or IHP SG13G2: transistors whose model, W or L changed, and nets that were **opened or shorted** |
| **Magic** | `.mag` | The same, with Magic's layer names, sub-cells from the same commit, and port changes |
| **ngspice** | `.raw` | Which signals changed and by how much, separating numerical noise; computed measurements with ngspice syntax |

It also checks a **layout against its schematic (LVS)** at any commit, and shows everything in a
**desktop viewer** with before/after views. Xschem, KLayout and Magic do not need to be installed.

## Installation

Riku ships as a single executable, `riku` (command line, interactive shell and viewer), for Linux x86_64
with glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+).

```bash
curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh
```

This installs the latest release into `~/.local/bin` and verifies its checksum. Variants:

```bash
curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh -s -- v0.2.0               # a specific version
curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sudo sh -s -- latest --system  # into /usr/local/bin
```

The `.tar.gz` and `.deb` packages are also on the [Releases](https://github.com/riku-chip/riku_chip/releases)
page. To build from source, see [`docs/dev/development.md`](https://github.com/riku-chip/riku_chip/blob/main/docs/dev/development.md).

## Try it in a minute

`riku demo` creates example projects with a real Git history in `~/riku-demos`, so you can explore Riku
without a design of your own:

```bash
riku demo
cd ~/riku-demos/ota
riku log --graph
```

```text
● 421f72d (HEAD, main, v1.0)  Rename the tail node: node -> tail
│   xschem/ota-5t.sch  3 components modified, 1 net added, 1 net removed
● d4e936d  Testbench: 2 pF load
│   sim/ota-5t_tb.raw  38 signals changed
│   xschem/ota-5t_tb.sch  1 component modified
● 6184836  Fix: the Vout route no longer touches Vp
│   layout/ota-5t.gds  1 component removed, 1 net modified
● 120ee0b  Layout: route Vout to the left edge
│   layout/ota-5t.gds  1 short, 1 component added
○   ad104a7 [merge]  Merge branch 'narrow-input-pair'
├─╮
● │ 6568371  Tidy up the schematic (move everything)
│ │   xschem/ota-5t.sch  (cosmetic changes only)
…
```

| Demo | What it contains |
|---|---|
| `ota` | A 5-transistor OTA in SKY130: schematic, testbench, layout and simulation. 11 commits, a branch, a short in the layout and its fix |
| `sram` | A 16×8 OpenRAM SRAM in SKY130: a sub-cell change seen in every instance, a renamed cell, metal fill on a branch |
| `inversor` | A 5 V inverter in Magic and Xschem: Magic layer names, a transistor changed inside its sub-cell, port classes, an open and its fix |
| `chip` | A 1 KB OpenRAM SRAM (9.9 MB GDS, 8,192 bitcells), to see how Riku performs on a large layout |

Each demo has a `README.md` with things to try. The [getting started guide](https://github.com/riku-chip/riku_chip/blob/main/docs/getting-started.md) walks through them.

## A quick tour

Riku works on any Git repository: you create and commit with `git`, Riku only reads. Every command also
works inside the interactive shell (`riku` with no arguments, with Tab completion).

**`riku log`** — the history, with what changed in each commit. Shorts and opens come first, in red.

**`riku show`** — the details of one commit. A parameter changed in the schematic:

```text
$ riku show HEAD~7
…
File     : xschem/ota-5t.sch
Changes  : 2

  ~ M1
      W: 2 → 4
  ~ M2
      W: 2 → 4
```

Transistors whose size changed in the layout, recognized with the PDK's own rules:

```text
$ riku show narrow-input-pair
File     : layout/ota-5t.gds
Changes  : 7

  - ota-5t:L65/20
      -1 polys / -3.050 µm²
  ~ ota-5t:sky130_fd_pr__nfet_01v8 @ (0.150, 3.367)
      w_um: 5.000 → 4.500
  …
```

**`riku diff`** — like `git diff`: the working tree against `HEAD`, or any two commits, branches or tags.

```bash
riku diff                                          # working tree vs HEAD, every file
riku diff v0.1 v1.0                                # everything between two tags
riku diff v0.1 v1.0 layout/ota-5t.gds -f visual    # one file, in the viewer
riku diff v0.1 v1.0 -f json                        # typed JSON for scripts and CI
```

In a simulation, a measurement computed with ngspice syntax:

```text
$ riku diff v0.1 v1.0 sim/ota-5t_tb.raw --expr "ac: a0 = max(db(v(vout)))"
  ~ a0
      = max(db(v(vout)))
      38.292 dB → 38.881 dB · Δ 0.589 dB (1.51 %)  (AC Analysis)
```

**`riku status`** — what changed on disk, classified as functional or cosmetic. It exits with `1` when
there are functional changes, so it fits in hooks and CI.

**`riku lvs`** — checks the layout against the schematic using transistor links stored next to the design
in `lvs/<cell>.toml`. `riku lvs --suggest` proposes the links it can deduce:

```text
$ riku lvs --suggest
Manual LVS  xschem/ota-5t.sch ↔ layout/ota-5t.gds (ota-5t) · worktree
  9 links suggested: M4, M3, M6, M7, M8, M9, M1, M2, M5
  → lvs/ota-5t.toml
  Linked: 9 of 9 schematic transistors · 24 of 24 in the layout
  different parameter in M1: W 4 ≠ 2
  …
```

**`riku open`** — the desktop viewer: before/after views of any diff, the commit graph (press **H**),
transistors and net highlighting on layouts, waveforms, and a side-by-side LVS view.

| More commands | |
|---|---|
| `riku render FILE -o image.png` | An image (PNG or SVG) of a file or of a version (`--rev`), without a window |
| `riku doctor` | Checks the environment: the repository, the PDK and which formats can be compared |
| `riku demo [--list]` | The example projects |
| `riku completions bash` | Shell completion for bash, zsh, fish, PowerShell or elvish |
| `riku about` | The authors and the university |

## Documentation

| Guide | |
|---|---|
| [Getting started](https://github.com/riku-chip/riku_chip/blob/main/docs/getting-started.md) | Install, the demos, and a guided tour |
| [Command reference](https://github.com/riku-chip/riku_chip/blob/main/docs/cli.md) | Every command and option |
| [Scripting and CI](https://github.com/riku-chip/riku_chip/blob/main/docs/scripting.md) | JSON output, exit codes, CI recipes |
| [Configuration](https://github.com/riku-chip/riku_chip/blob/main/docs/configuration.md) | `.riku.toml`, environment variables, PDK discovery, caches |
| [Viewer](https://github.com/riku-chip/riku_chip/blob/main/docs/viewer.md) | The desktop viewer |
| [Formats](https://github.com/riku-chip/riku_chip/blob/main/docs/formats.md) | What is compared in each file format, and the limits |
| [LVS](https://github.com/riku-chip/riku_chip/blob/main/docs/lvs.md) | Layout versus schematic, at any commit |

For contributors: [CONTRIBUTING.md](https://github.com/riku-chip/riku_chip/blob/main/CONTRIBUTING.md),
[architecture](https://github.com/riku-chip/riku_chip/blob/main/docs/dev/architecture.md),
[development](https://github.com/riku-chip/riku_chip/blob/main/docs/dev/development.md),
[design notes](https://github.com/riku-chip/riku_chip/blob/main/docs/dev/design-notes.md) and the
[roadmap](https://github.com/riku-chip/riku_chip/blob/main/docs/dev/roadmap.md). Release history:
[CHANGELOG.md](https://github.com/riku-chip/riku_chip/blob/main/CHANGELOG.md).

## Status

Riku is **alpha**. The three file types work end to end, and the command-line output and JSON schemas are
versioned. Expect rough edges; [issues](https://github.com/riku-chip/riku_chip/issues) and pull requests
are welcome.

## Authors

<img src="docs/img/uni-logo.png" alt="Seal of the National University of Engineering (UNI)" width="100">

Riku was made at the **National University of Engineering (UNI)**, Lima, Peru, by:

- **Carlos Cueva**, Electronic Engineer
- **Amado Frias**, Electronic Engineer

`riku about` (and the interactive shell when it starts) shows the same credits with the UNI seal.

## License

[Apache-2.0](LICENSE), the same as [`xschem-viewer-rust`](https://github.com/carloscl03/xschem-viewer-rust),
the Xschem engine. The layout engine, [`gdstk_rust`](https://github.com/Adriel2503/gdstk_rust), keeps
gdstk's license (Boost Software License 1.0), which is compatible.
