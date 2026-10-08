# File formats

This page covers what Riku compares in each file format, what it treats as cosmetic, and the limits you can run into.

For how to run the comparisons, see [cli.md](cli.md) and [viewer.md](viewer.md). The JSON form of every change is described in [scripting.md](scripting.md).

On this page:

- [Xschem schematics](#xschem-schematics-sch)
- [Layouts: GDSII, OASIS and Magic](#layouts-gdsii-oasis-and-magic)
- [Magic layouts](#magic-layouts-mag)
- [Transistors and nets](#transistors-and-nets)
- [Layer styling in the viewer](#layer-styling-in-the-viewer)
- [ngspice simulations](#ngspice-simulations-raw)
- [Expressions](#expressions)

The examples come from the demos that `riku demo` creates in `~/riku-demos/<name>` (`ota`, `inversor`, `sram`). You can run the same commands there and get the same output.

## Xschem schematics (`.sch`)

Riku reads `.sch` files with its own Xschem parser, so you don't need `xschem` installed. A file is treated as an Xschem schematic when its header contains `xschem version=`.

### What is compared

- **Components, by instance name** (`M3`, `R1`, `x1`). Riku reports components that were added, removed, renamed or modified. For a modified component it shows the symbol and every parameter that changed, with the value before and after.
- **Renames.** If one component disappears and another appears with the same symbol and at least 80% of the same parameters, Riku reports a rename instead of a removal plus an addition.
- **Nets, by name.** Riku lists the nets that were added or removed. That includes nets Xschem names automatically, such as `#net1`. When you rename a label, the old net shows as removed and the new one as added.
- **New or deleted files.** If the file is new or was deleted, everything in it appears as added or removed.

> [!NOTE]
> Pin connections are not compared one by one. If you rewire a component pin from one existing net to another existing net, and no net name appears or disappears, the change is not reported. Use [`riku lvs`](lvs.md) to check connectivity against the layout.

From the `ota` demo, commit `0f05efe` ("Narrower input pair: M3, M4 W 20u -> 18u"):

```text
$ riku show 0f05efe
…
File     : xschem/ota-5t.sch
Changes  : 2

  ~ M3
      W: 20 → 18
  ~ M4
      W: 20 → 18
```

### What counts as cosmetic

- A component whose only changes are its **position, rotation or mirroring** is a cosmetic change. Placement never counts as a parameter.
- **Move All.** If more than 80% of the components that exist in both versions only moved, Riku adds a cosmetic "the whole schematic (Move All)" entry. A tidy-up commit like that has no semantic changes:

```text
$ riku show 6568371
…
    Tidy up the schematic (move everything)

File     : xschem/ota-5t.sch
  no semantic changes
```

Cosmetic changes are counted in the file header (`Cosmetic : 21`), but they don't make `--ci` report functional changes.

### Hierarchy

An instance goes down to a **sub-schematic** when the project contains that sub-schematic's `.sch`. Riku uses the file named in the instance's `schematic=` attribute or, when there is none, the symbol's file with a `.sch` extension (`amp.sym` → `amp.sch`). It looks for that file next to the schematic that uses it, then from the repository root. It always reads from the **same version** being compared, so each side of a diff uses its own sub-schematics.

If a sub-schematic changed in that version, the instance that uses it is reported as modified. This applies whether the sub-schematic itself changed, its `.sym` changed (different pins change how the instance connects), or something deeper in the hierarchy changed. From `riku diff v0.1 v1.0` in the `ota` demo:

```text
File     : xschem/ota-5t_tb.sch
Changes  : 2

  ~ C1
      value: 1p → 2p
  ~ x1
      changed inside: xschem/ota-5t.sch
```

If every change inside the sub-schematic is cosmetic (for example a Move All), the instance change is cosmetic as well. PDK symbols don't have a `.sch` in the project, so they are leaves.

### Symbols (`.sym`) and the PDK

You can open `.sym` files in the viewer, but Riku doesn't compare them on their own. `riku show` lists them under "No Riku module". As described above, a changed `.sym` still marks the instances that use it as changed.

Riku needs the symbols to draw a schematic and to work out which pin goes to which net. It looks for them in this order: the project (at the same version), your `.xschemrc`, the PDK selected by `$PDK_ROOT` and `$PDK`, and finally the installed PDK that has the symbols the schematic uses. For the full lookup rules and the variables involved, see [configuration.md](configuration.md). `riku doctor` and the viewer's **Details** panel show where the symbols came from and warn when any are missing.

### Errors

If a side isn't valid UTF-8 or isn't an Xschem file, Riku reports that file as an error. It does not report "everything removed".

## Layouts: GDSII, OASIS and Magic

Riku compares `.gds`, `.oas` and `.mag` files with the same engine. It detects the format from the file content (the GDSII header record, the `%SEMI-OASIS` signature, or a first line of `magic`). So the same design saved as GDSII and as OASIS, or saved with different database units, compares without changes.

### Geometry: XOR by cell and layer

For each cell that exists in both versions, Riku flattens the hierarchy and computes the XOR of every layer. For each change it reports:

- the cell and the layer (`ota-5t:L70/20`), and the sub-cell the geometry came from, if any;
- the number of polygons added and removed, and the area added and removed in µm²;
- the bounding box of the change in that cell's coordinates.

Because the comparison is by area, re-cutting the same shape into different polygons is not a change.

A change inside a sub-cell appears in the sub-cell itself and in every cell that instantiates it, with its origin. From the `sram` demo, commit `4437d9e` ("Bitcell: 20 nm wider bitline metal2"):

```text
$ riku show 4437d9e
…
  + sky130_fd_bd_sram__sram_sp_cell_met2:L69/20
      +3 polys / +0.086 µm²
      -0 polys / -0.000 µm²
      bbox: (-0.010, 0.285) → (1.210, 1.295) µm
…
  + sram_16x8_sky130_sky130_bitcell_array:L69/20:sky130_fd_bd_sram__sram_sp_cell_opt1
      origin: sram_16x8_sky130_sky130_bitcell_array → sky130_fd_bd_sram__sram_sp_cell_opt1 (in 79 instances)
      +271 polys / +4.553 µm²
      -0 polys / -0.000 µm²
      bbox: (-0.010, 0.285) → (21.210, 26.575) µm
…
```

### Instances and arrays

Every instance of a changed sub-cell gets its own change, and so does every repetition of an array reference (AREF). In the viewer, each one has its own box that you can click. The text output groups the instances that share a cell, layer and sub-cell. It shows `origin: … @ (x, y)` for a single instance and `(in N instances)` for a group. In JSON, the count is in `element.via.instances`.

### Cells: added, removed, renamed

- Riku reports cells that exist on only one side as `+ cell:NAME` or `- cell:NAME`.
- **Pure renames.** If a cell disappears and a cell with exactly the same flattened geometry appears, Riku reports a rename. A cheap fingerprint proposes the pair and an XOR confirms it:

  ```text
  $ riku show c6cf2ee
  …
    r cell:sram_16x8_sky130_pmos_m1_w1_120_sli_dli_da_p → sram_16x8_sky130_pmos_w1p12_da
  ```

  If a renamed cell also changed geometry, it shows up as one cell removed and another added. If several cells share the same fingerprint, Riku doesn't guess. Cells with no geometry are never paired.
- KLayout's metadata cell `$$$CONTEXT_INFO$$$` is ignored. A file saved with KLayout and one saved without it compare the same.

### Cell libraries

A file with many top cells (a standard-cell library, for example) is compared cell by cell. A cell counts as changed when its own geometry changed or when the change came from one of its sub-cells. The viewer's cell list can show only the changed cells.

### Layer names

GDSII layers are shown as `L<layer>/<datatype>`. If an OASIS file names its layers (`LAYERNAME`), or for Magic layouts, every change also carries the layer name (`layer_name` in JSON). In text output, Magic changes use the name directly (`inv:metal2`).

### Cosmetic threshold

A geometry change is cosmetic when the area it adds plus the area it removes is below the threshold. The default is **0.01 µm²**, well below the minimum feature area of PDKs such as SKY130 and GF180MCU. That catches grid-snapping noise and rounding slivers without hiding real edits. Riku checks each reported item (cell, layer and sub-cell) against the threshold on its own.

| Where | Setting |
|---|---|
| Command line | `--cosmetic-threshold-um2 0.05` (on `riku diff` and `riku show`) |
| `.riku.toml` | `[layout]` `cosmetic_threshold_um2 = 0.05` |

The command-line flag wins over the file. Other formats ignore this setting.

### Errors and limits

- **Broken files.** If a side isn't a layout, is truncated, or contains a cell cycle (A instantiates B, which instantiates A), Riku reports that file as an error. It never reports it as "no changes".
- **Missing cells.** If a reference points to a cell that isn't in the file (common with partial stream-outs), Riku warns and lists those cells. Those instances aren't compared, and the rest of the file is.
- **Boolean failures.** If the polygon XOR fails on a layer, Riku warns that the change for that layer may be incomplete.
- **File size.** Riku skips files larger than **50 MiB** in any version with a message ("… MB, more than the 50 MB limit; not compared"). The limit applies to every format and can't be changed.

### Diff cache

Riku stores the result of comparing large layouts on disk, so the next `riku diff`, `riku log`, or the same diff opened again in the viewer reads it back instead of recomputing it.

- **When:** only if the files read for the comparison add up to more than 1 MiB. Small diffs take milliseconds anyway.
- **Key:** the version of the layout engine, the bytes of every file read (including the Magic sub-cells of each side), and the parameters (cosmetic threshold, Magic lambda). If anything changes, the result is recomputed.
- **Where:** `$RIKU_CACHE_DIR`, or the system cache directory (`~/.cache/riku/diff` on Linux). Net extraction summaries (see [below](#per-cell-extraction)) are stored next to it, in `nets/`.
- **Size:** each directory is capped at 512 MiB. When a directory goes over the cap, Riku deletes the oldest entries. If an entry is unreadable, Riku deletes it and recomputes the result without reporting an error.
- **Off:** `--no-cache` or `RIKU_NO_CACHE=1`.

See [configuration.md](configuration.md) for all cache-related variables.

## Magic layouts (`.mag`)

Magic stores **one cell per file**, and its layers are **logical, named types** (`ndiff`, `poly`, `locali`, `metal1`) rather than mask layers. Riku compares those types, which are the ones you edit, and doesn't convert them to GDS through the `.tech` file. So a change reads `inv:metal2` instead of a GDS layer number:

```text
$ riku show 77de166
…
    Layout: wider metal2 on out

File     : layout/inv.mag
Changes  : 1

  + inv:metal2
      +2 polys / +0.307 µm²
      -0 polys / -0.000 µm²
      bbox: (1.500, -2.100) → (2.010, 0.970) µm
```

### Sub-cells from the same version

Riku resolves each `use` line by looking for `<cell>.mag` in this order:

1. The directory written in the `use` line, if there is one. It can be relative to the file that uses the cell, or start with `$PDK_ROOT`, `$PDKPATH` or `~`. If `$PDKPATH` isn't set, it means `$PDK_ROOT/<tech>`.
2. Next to the file that uses the cell, **in the same commit** (or on disk for `riku status`).
3. The directories in `$RIKU_MAG_PATH` (separated like `PATH`), and the PDK libraries of the file's `tech` line (`$PDK_ROOT/<tech>/libs.ref/*/mag`).

So if you change only a sub-cell file, the change also shows in every file that uses it, in that version. From the `inversor` demo, commit `e08504b` widens the NMOS inside its own sub-cell file:

```text
$ riku show e08504b
…
File     : layout/inv.mag
Changes  : 19

…
  + inv:mvnmos:sky130_fd_pr__nfet_g5v0d10v5_H9JWFY
      origin: inv → sky130_fd_pr__nfet_g5v0d10v5_H9JWFY @ (1.350, -2.865)
      +1 polys / +0.250 µm²
      -0 polys / -0.000 µm²
      bbox: (1.100, -2.020) → (1.600, -1.520) µm
…
  ~ sky130_fd_pr__nfet_g5v0d10v5_H9JWFY:sky130_fd_pr__nfet_g5v0d10v5 @ (0.000, 0.104)
      w_um: 2.000 → 2.500

File     : layout/sky130_fd_pr__nfet_g5v0d10v5_H9JWFY.mag
Changes  : 10
…
```

If Riku can't find a cell, it leaves the cell empty, warns, and compares the rest. Re-saving a cell in Magic only updates its timestamps, so it gives `no semantic changes`.

### Units: lambda and `magscale`

Lambda comes from the technology, not from the file: Riku reads it from the `.tech` of the installed PDK (`scalefactor` in `cifoutput`). Without that `.tech`, it uses 0.01 µm for SKY130 and IHP SG13G2 and 0.05 µm for GF180MCU. For any other technology it uses 0.01 µm and warns. `RIKU_MAG_LAMBDA` (in µm) forces a value.

Files in one hierarchy can use different `magscale` values. Riku puts them all on one common grid, so coordinates stay exact.

### Layers left out

Magic types that aren't mask data are not compared: DRC marks and router hints (`checkpaint`, `checksubcell`, `error_p`, `error_s`, `error_ps`, `magnet`, `fence`, `rotate`). Labels on those layers are kept.

### Ports

Riku compares each cell's ports by name. It reports ports that were added or removed, and changes to a port's class, use, index, sides or layer. From the `inversor` demo:

```text
$ riku show 721c1e3
…
    Layout: in is an input port, out an output

File     : layout/inv.mag
Changes  : 2

  ~ inv:port:in
      class: — → input
      use: — → signal
  ~ inv:port:out
      class: — → output
      use: — → signal
```

A port that only moved is a cosmetic change.

How Riku reads Magic files was checked against KLayout's reader. For every bundled SKY130 and GF180MCU hierarchy, both give the same polygon count and area per layer. You can rerun that check with `tools/verify/mag/compare_mag.sh` (see [development.md](dev/development.md#magic)).

## Transistors and nets

For a layout in **SKY130, GF180MCU or IHP SG13G2** (GDSII, OASIS or Magic), Riku recognizes transistors and resistors and builds the nets: which metal, poly and diffusion are joined through contacts and vias. With that, a layout diff tells you more than "this polygon changed". It tells you which transistor got wider and which nets were shorted or opened.

### Where the rules come from

Riku has no hand-written tables per PDK. It derives everything from the **Magic `.tech` file of the installed PDK** (under `$PDK_ROOT`). If that PDK isn't installed, it uses a copy of the same rules compiled into Riku. Riku picks the PDK from the layout's layers (and, for Magic, from its type names), not from the file name.

| From the `.tech` | Used for |
|---|---|
| `cifinput` (its first style) | Which GDS layers make up each Magic type (an `nfet` is diffusion and poly and N+ implant without P+ implant, and so on), with `grow` and `shrink`. Also which GDS layers carry each type's labels and pins. |
| `extract`: `device` lines | The SPICE model of each transistor and resistor type, chosen by W and L the way Magic chooses it (in SKY130, a narrow `nfet` becomes `special_nfet_01v8`). Also which types are the source/drain and the body. |
| `extract`: `substrate` | Which types are the substrate. |
| `contact`, `connect`, `aliases` | Which types conduct together when they touch, and what each contact joins. |
| `types` | Alternative names for each type, so a `.mag` can use any of them. |

### Transistors

- Each **gate** region (diffusion ∩ poly) is one finger. Its model is the `.tech` rule that holds at a point inside the gate.
- **W and L** are measured the way KLayout measures them. The source and drain are the two diffusion regions that touch the gate. `W` is half the total length of the edges where the gate touches them, and `L = gate area / W`. This is exact for a rectangle and the standard convention for a bent gate. A gate that doesn't touch exactly two diffusion regions isn't a transistor.
- In Magic files, transistors are already painted as their own types (`nfet`, `mvnmos`), so Riku reads them directly.

Transistors from the two versions are paired by position: two are the same transistor if one's gate contains a point inside the other's. A transistor that grew is still the same transistor. One that moved farther than its own size counts as removed and added. A paired transistor is reported if its model, W or L changed by more than 0.5 nm. From the `ota` demo, commit `02496a4`:

```text
$ riku show 02496a4
…
    Layout: trim the input pair diffusion to match

File     : layout/ota-5t.gds
Changes  : 7

  - ota-5t:L65/20
      +0 polys / +0.000 µm²
      -1 polys / -3.050 µm²
      bbox: (-0.400, 5.600) → (5.700, 6.100) µm
  ~ ota-5t:sky130_fd_pr__nfet_01v8 @ (0.150, 3.367)
      w_um: 5.000 → 4.500
  ~ ota-5t:sky130_fd_pr__nfet_01v8 @ (1.150, 3.367)
      w_um: 5.000 → 4.500
…
```

Riku only looks for transistor changes in cells where a layer used by the transistor rules changed (diffusion, poly, implants, wells, or the matching Magic types). If only metal changed, no transistor could have changed.

### Nets

- **Connectivity.** Riku evaluates each type as a region with its `.tech` rule, including `grow` and `shrink`, and in Magic's paint order (on one plane, a later type covers an earlier one). Pieces of types that `connect` joins and that touch are one net. The substrate is a net even where nothing is drawn.
- **Names.** A label names the net it lands on. It is a **pin** if it sits on a pin layer, or, in Magic, if it is a port. A net with two labels (after a short) is shown as `Vout = Vp`. A net with no label is identified by a transistor terminal or a resistor that touches it, with its position.
- **Resistors.** Riku recognizes the resistor body between its two terminals (`device resistor` in the `.tech`). Resistors whose model is `None` (metal resistors in IHP SG13G2) are treated as shorts.
- **Not yet supported:** diodes and capacitors.

### Opens, shorts and renames

Nets have no identity of their own, so Riku compares them through **anchors** that exist in both versions: labels (by text), the terminals of transistors paired by position, and, when comparing by cells, the sub-cell instances that didn't change. Each anchor links the net it is on in A with the net it is on in B.

| Pattern | Reported as |
|---|---|
| Several nets of A became one net of B | `!` **short** (nets joined) |
| One net of A became several nets of B | `!` **open** (a net split) |
| A net with several labels (a short) split so each part keeps one label | `~` nets separated (short fixed) |
| The same net with a different label | `r` rename |

Something that exists on only one side, such as a new transistor, isn't an anchor. Adding a transistor isn't a short.

Opens and shorts are **errors**. They are listed first in the file's changes, and in JSON they carry `"severity": "error"`. From the `ota` demo, commit `120ee0b` routes `Vout` across `Vp`:

```text
$ riku show 120ee0b
…
    Layout: route Vout to the left edge

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

The next commit fixes it:

```text
$ riku show 6184836
…
  ~ ota-5t:net:Vout = Vp
      nets separated (short fixed): Vout = Vp → Vout, Vp
      bbox: (-5.600, -0.560) → (-3.460, 9.150) µm
  - ota-5t:L70/20
      +0 polys / +0.000 µm²
      -1 polys / -4.580 µm²
      bbox: (-5.600, -0.560) → (-3.460, 9.150) µm
```

**Where Riku reports them.** Riku compares nets in every cell that has its own change on a conducting layer, and in those cells' ancestors, so it finds a short that only appears once the parent's wiring is included. Each open or short is reported once, in the **lowest cell** where it appears, not repeated in every cell above it. The `bbox` is the area of the geometry changes that touch the affected nets.

### Per-cell extraction

Riku extracts nets **cell by cell** and remembers the result, so that large hierarchical layouts stay fast:

- **Each cell is extracted once, on its own.** Riku extracts a cell's own geometry together with its small sub-cells (under 256 flattened polygons, such as contact or single-transistor cells). Those are folded into the parent the way Magic's `gds flatglob` does, because a contact by itself means nothing. A parent then only has to join what touches its children, and children that touch each other. Identical neighbors, such as the cells of an array, are worked out once.
- **Context is respected.** The parent handles anything that depends on what's around a cell. That includes rules that merge nearby shapes (`grow` then `shrink`, such as wells close enough to join), and the substrate (a deep N-well drawn in the parent isolates the wells of its children).
- **Results are remembered by content.** Riku keys each cell's summary by a fingerprint of its geometry, labels, Magic ports and rules. An identical cell, whether on the other side of the diff, in another commit of `riku log`, or in a later run, is never extracted again. Summaries are kept in memory (512 MB by default; `RIKU_NETS_MEM_MB` changes the cap) and on disk next to the [diff cache](#diff-cache), for cells that took more than a few milliseconds to build.
- **Same result as flat.** Flattened, the per-cell result gives the same netlist as extracting the whole cell flat (checked by `tools/verify/nets/hier.sh`).

In practice, a standard cell takes milliseconds. On a large macro, only the cells that actually changed, and the path from them up to the top, are extracted again.

**Very large cells.** If a cell has more than **2 million polygons** when flattened (a whole chip, for example), Riku only compares its nets when its extraction is already in memory or on disk. Otherwise it warns, and it still compares nets in the cell's sub-cells. Set `RIKU_FULL_NETS=1` to extract such a cell anyway, which takes seconds the first time and is fast after that. In the viewer, the transistors and nets of a cell that size are counted but not drawn, because drawing them requires flattening.

`RIKU_FLAT_NETS=1` switches back to extracting each changed cell flat. Use it only for comparison or troubleshooting.

### Accuracy

The scripts in `tools/verify` (see [development.md](dev/development.md#verification)) check Riku against reference tools on the standard-cell libraries of all three PDKs:

- **Transistors** (model, W and L of each finger) match each PDK's reference netlist, both from the GDS and, where the PDK ships them, from the `.mag` cells. The few cells listed as exceptions in the script are ones where the PDK netlist doesn't match its own layout, and KLayout's extractor agrees with Riku.
- **Nets** match the reference netlists in a full topological comparison with **Netgen**. The exceptions are listed in the script. In each of them, Magic's own extraction gives the same result as Riku.
- **A whole macro.** The 1 KB SRAM macro of the `chip` demo, extracted cell by cell, was compared against Magic on the full macro (`tools/verify/nets/chip_vs_magic.sh`). The known difference is listed in the [roadmap](dev/roadmap.md).

How the extraction works internally is described in [architecture.md](dev/architecture.md) and [design-notes.md](dev/design-notes.md#hierarchical-net-extraction).

## Layer styling in the viewer

The viewer draws each layer with the color, name and stacking order of its PDK, without any per-project setup:

- SKY130, GF180MCU and IHP SG13G2 have curated tables built into Riku (role and stacking chosen for each layer). These work even without the PDK installed.
- For any other PDK, Riku reads the KLayout `.lyp` of the installed PDK (`$PDK_ROOT`, `$PDKPATH`). A new PDK is drawn correctly without recompiling Riku.
- Each layer has a role: device and interconnect layers are filled, wells get a faint tint, and implants, markers and pins are drawn as outlines only. Unknown layers get a color from a generic palette, and are drawn as an outline if their datatype looks like a pin, label or marker.
- Magic types take the style of their equivalent GDS layer.

See [viewer.md](viewer.md) for the diff view, the **Transistors** layer, net highlighting and the rest of the layout viewer.

## ngspice simulations (`.raw`)

Riku reads ngspice result files (Berkeley SPICE3 `.raw`, binary or ASCII). That way you can version simulation results next to the circuit and see how the behavior changed.

### What is compared

- **Analyses.** A `.raw` file holds one or more analyses (`op`, `tran`, `ac`, `dc`, `noise`…). Riku pairs them between the two versions by name (`Plotname`) and by order among analyses with the same name.
- **Signals.** Riku matches signals by name (ignoring case) and reports the ones that are new (`new in AC Analysis`), the ones that are gone, and how much each one changed.
- **Different time steps.** Two runs rarely use the same time steps, so Riku compares each signal on the **union of both grids**, within their common range, interpolating each version linearly. For each signal it reports:

| Metric | Meaning |
|---|---|
| `Δmax … at …` | Largest absolute difference \|B − A\|, and the x value (time, frequency, sweep) where it happens |
| `RMS` | RMS of the difference over the x range |
| `% of range` | Δmax divided by the signal's range (max − min over both versions) |

- **Operating point.** A single-point analysis (`op`) is compared directly. Its "range" is the larger magnitude of the two values.
- **Complex analyses** (`ac`, `noise`) are compared in magnitude, in **dB**. The frequency axis uses its real part.
- **Duration change.** If the x range changed (a longer transient, for example), Riku compares the common part and warns ("the axis changed from … to …; the common range is compared"). If the two ranges don't overlap at all, Riku reports the signal as modified, with "no common axis to compare".

From the `ota` demo, commit `d4e936d` ("Testbench: 2 pF load"):

```text
$ riku show d4e936d
…
File     : sim/ota-5t_tb.raw
Changes  : 38
Cosmetic : 88

…
  ~ v(vout)
      Δmax 5.971 dB at 31.623 MHz · RMS 5.961 dB · 9.64 % of range  (AC Analysis)
…
```

### Tolerance

A signal change is cosmetic when Δmax is at most **0.1% of the signal's range**. An absolute floor of 1e-12 keeps numerical noise on near-constant signals from being reported.

| Where | Setting |
|---|---|
| Command line | `--tolerance 0.5%` or `--tolerance 0.005` (on `riku diff` and `riku show`) |
| `.riku.toml` | `[waveform]` `tolerance = "0.5%"` (or `0.005`) |

The value must be strictly between 0 and 1 (0% to 100%). The command-line flag wins over the file.

In JSON, each signal is a change with `element.type = "signal"`, its `plot` and `name`, and the details `max_abs_diff`, `at`, `x_unit`, `rms_diff`, `rel_diff` and `unit`. See [scripting.md](scripting.md).

### Expressions

Besides the signals in the file, Riku can compare **computed signals** written in ngspice syntax. Pass `--expr` (you can repeat it) on `riku diff`, `riku show` and `riku render`. To compare an expression every time, list it under `expressions` in `.riku.toml`. Expressions from `--expr` are added to the ones in the file.

```bash
riku diff HEAD~1 HEAD sim/tb.raw --expr "gain = v(out)/v(in)" --expr "tran: vpk = max(v(out))"
```

```toml
[waveform]
expressions = ["gain = v(out)/v(in)", "tran: vpk = max(v(out))"]
```

Riku compares an expression that gives a signal like any other signal (Δmax, RMS, % of range). It compares one that gives a single number directly. From the `ota` demo, the DC gain between the first commit and the last:

```text
$ riku diff 5338fc4 HEAD sim/ota-5t_tb.raw --expr "ac: a0 = max(db(v(vout)))"
File     : sim/ota-5t_tb.raw
Changes  : 76
Cosmetic : 51

…
  ~ a0
      = max(db(v(vout)))
      38.292 dB → 38.881 dB · Δ 0.589 dB (1.51 %)  (AC Analysis)
```

For a number, the percentage is Δ divided by the larger of the two magnitudes, and the tolerance applies to that same ratio.

| What | Syntax |
|---|---|
| Signals | `v(out)`, `v(a,b)` (= `v(a) − v(b)`), `i(v1)`, `@m1[id]`, `time`, `frequency`, or any variable name in the file. A bare node name is `v(node)`: `out`, `x1.node`. Names ignore case. |
| Numbers | `1.5`, `1e-9`, SPICE suffixes `f p n u µ m k meg g t mil` (`10u`, `2meg`). Letters after a suffix are ignored (`100nF`). |
| Constants | `pi`, `e` |
| Operators | `+ - * / ^`, parentheses, unary `-`. `^` binds tighter than unary minus and is right-associative: `-v^2` = `-(v^2)`, `2^3^2` = `2^9`. |
| Point by point | `abs` (alias `mag`), `real` (`re`), `imag` (`im`), `ph` (`phase`, in degrees), `db` (20·log10 of the magnitude), `sqrt`, `exp`, `ln` (natural log), `log` (`log10`, base 10), `sin`, `cos`, `tan`, `atan`, `max(a, b)`, `min(a, b)` |
| Calculus | `deriv(v)` (derivative along the x axis), `integ(v)` (running integral, trapezoidal) |
| Single numbers | `max(v)`, `min(v)`, `pp(v)` (max − min), `mean(v)` (alias `avg`, average over the x axis), `rms(v)`, `integral(v)` (total integral), `length(v)` (number of valid points), `at(v, x)` (value at `x`, interpolated) |
| Indexing | `v[0]`, `v[-1]` (last point), `v[10:20]` (points 10 to 20, inclusive), `window(v, x0, x1)` (only the points with x between `x0` and `x1`) |
| Name | `gain = …` names the result. Without a name, the expression text is the name. |
| Analysis | `op:`, `tran:`, `ac:`, `dc:`, `noise:`, `tf:`, `sens:`, `pz:`, `disto:` limit the expression to that analysis |

Rules worth knowing:

- **Where an expression applies.** Without an analysis prefix, an expression is evaluated in every analysis that has all of its signals. If it applies nowhere, or gives no valid value anywhere, Riku warns ("gives no values in any analysis"). It also turns syntax errors and evaluation errors (an index out of range, `at()` outside the axis, an unknown function) into warnings, so they never stop the diff.
- **Complex values.** In `ac`, operations use the complex values (`v(out)/v(in)` divides complex numbers), and a complex result is shown in dB, like the file's own signals. `db()` and `ph()` give real results in dB and degrees. With complex inputs, `max`, `min` and `pp` compare magnitudes, and `atan` uses the real part. If an expression gives a single complex number (`v(out)[0]` in `ac`), Riku compares its linear magnitude, without a unit.
- **Units** are kept when Riku can work them out: a voltage or current through scaling, sums of the same kind, indexing and reductions; dB for `db()`; degrees for `ph()`.
- **Skipped points.** Points outside a slice or `window()`, and points that can't be computed (a division by zero, for example), are not compared. JSON reports how many as `skipped_points`.

In the viewer, the **Expressions** field in **Details** adds expressions as curves or as measurements. See [viewer.md](viewer.md).
