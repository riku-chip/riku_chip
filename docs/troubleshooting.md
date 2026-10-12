# Troubleshooting

Common problems, what causes them and what to do. Run `riku doctor` first: it shows the repository, the PDK, the format modules and Netgen as Riku sees them. Every setting mentioned here is described in [configuration.md](configuration.md).

**On this page:** [Setup and PDK](#setup-and-pdk) · [Schematics](#schematics) · [Layouts](#layouts) ·
[Simulations](#simulations) · [LVS](#lvs) · [Images and the viewer](#images-and-the-viewer) · [Git](#git)

## Setup and PDK

| Symptom | Cause | What to do |
|---|---|---|
| `riku: command not found` after installing | `~/.local/bin` is not on your `PATH` | Add it (the installer prints the line for your shell), or install with `--system` (see [Getting started](getting-started.md#install)) |
| `riku doctor` shows no PDK | `PDK_ROOT` and `PDK` are not set, and there is no `.xschemrc` | In iic-osic-tools: `sak-pdk sky130A` (or your PDK). Elsewhere: `export PDK_ROOT=… PDK=…` |
| Messages are in English and you want Spanish | The default language is English | `export RIKU_LANG=es`; in the viewer, **Settings → Language** |

## Schematics

| Symptom | Cause | What to do |
|---|---|---|
| Symbols are empty boxes; **Details** lists missing symbols | Riku cannot find the `.sym` files | Set the PDK (above). For project symbols, see [Xschem symbols](configuration.md#xschem-symbols) |
| `riku diff` on a `.sch` reports pins or nets differently from the viewer | The text and JSON diff do not use the project's symbols: they read `.xschemrc` only from the current folder or your home folder (a project file called `xschemrc`, without the dot, is not read) | Run from the folder that has your `.xschemrc`, or make the project symbols reachable through it ([details](formats.md#symbols-sym-and-the-pdk)) |
| A rewired pin is not reported | Nets are compared by name, not pin by pin | Expected; see [Formats → Xschem](formats.md#what-is-compared). `riku lvs` checks connectivity against the layout |
| `not a xschem file (another format with the same extension); not compared` | The `.sch` is from another tool (KiCad, Qucs-S) | Expected: Riku reads Xschem schematics only |

## Layouts

| Symptom | Cause | What to do |
|---|---|---|
| A Magic (`.mag`) diff shows geometry but no transistor or net changes | Without the PDK installed, Riku only recognizes a `.mag` whose path names the PDK (`sky130_fd_pr__…`); for the rest it needs the PDK's Magic `.tech` | Install the PDK the layout uses (`sak-pdk <pdk>` in iic-osic-tools) and check `riku doctor` ([details](formats.md#where-the-rules-come-from)) |
| A Magic layout has empty sub-cells, with a warning (`compared as empty`) | The `.mag` uses PDK library cells (`sky130_fd_sc_hd__…`) that were not found | Install the PDK, or list the cell folders in `RIKU_MAG_PATH` |
| No transistors or nets, and **Summary** says to open a sub-cell | The cell has more than 2 million flattened polygons (a whole chip) | Open or compare a sub-cell (`--cell`) |
| `more than the 50 MB limit; not compared` | Files over 50 MB are not read | Not configurable. Compare a smaller export of the block you changed |
| Labels or warnings in Spanish while the rest is in English | Riku 0.2.1 or earlier: some layout texts were not translated | Update Riku |

## Simulations

| Symptom | Cause | What to do |
|---|---|---|
| A waveform image does not show the signal you want | Without `--signal`, an image draws at most 3 signals | `--signal "v(out)"` and, if needed, `--analysis tran` (see [Images](cli.md#images-riku-render-and--f-pngsvg)) |
| An `--expr` does not appear in an image | It gives a single number (a measurement), not a curve | Expected: numbers are in the text and JSON diff |
| You want to compare two `.raw` files that are not in Git | `riku diff` compares versions in Git | In the viewer: **Compare with**. From the command line, commit both runs (a scratch branch is enough) |

## LVS

| Symptom | Cause | What to do |
|---|---|---|
| `riku lvs` says nothing is linked | There is no `lvs/<cell>.toml` yet | `riku lvs --suggest`, then link the rest in the viewer ([workflow](lvs.md#workflow)) |
| `riku log --lvs` or `status --lvs` says the LVS is not available | They use Netgen, not your links file | Install Netgen (it comes with iic-osic-tools), or use `riku lvs --log` for the history of the links |
| `no schematic with a layout to compare` | No schematic and layout share a name, and `.riku.toml` has no `[[lvs]]` | `riku lvs --sch … --layout …`, or add `[[lvs]]` ([details](lvs.md#which-schematic-goes-with-which-layout)) |

## Images and the viewer

| Symptom | Cause | What to do |
|---|---|---|
| `the viewer needs a graphical desktop` | No `DISPLAY` or `WAYLAND_DISPLAY` (SSH, CI, a container without a desktop) | Use the container's desktop (VNC or X11 forwarding), or `riku render` / `-f png` for images |
| An image has no text | No system fonts (minimal containers) | Install a font package (`fonts-dejavu-core` on Debian/Ubuntu) |
| `diff` or `show` does not accept `--net` or `--show-devices` | They draw a diff, and nets and the Transistors layer are drawn for one version | `riku render FILE --rev REV --net NAME` (or `--show-devices`) |
| The window opens off-screen | A monitor was unplugged | Delete the `"window"` entry from `~/.local/share/riku-gui/app.ron` ([details](viewer.md#window-and-preferences)) |

## Git

| Symptom | Cause | What to do |
|---|---|---|
| `stored with Git LFS … not compared` | The file is in Git LFS: Git holds a small pointer, not the file | Not supported yet. Keep the files you want to compare out of LFS |
| `FILE: not in A nor in B` | The path is wrong, or relative to another folder | Paths are relative to where you are, as in Git ([Conventions](cli.md#conventions)) |
| The initial commit cannot be opened with `-f visual` | It has no parent to compare with | `riku open FILE`, or `riku render FILE --rev COMMIT` |
