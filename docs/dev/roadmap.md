# Roadmap

Known limitations of the current release, the improvements already scoped, and ideas that are not planned yet.

Nothing here is a promise or a schedule. If you want to work on an item, open an issue first so the approach can be agreed; see [CONTRIBUTING](../../CONTRIBUTING.md). Background on many of these items is in the [design notes](design-notes.md).

On this page:

- [Known limitations](#known-limitations)
- [Planned improvements](#planned-improvements)
- [Upstream (xschem-viewer-rust)](#upstream-xschem-viewer-rust)
- [Only if needed](#only-if-needed)
- [Ideas](#ideas)

## Known limitations

### Language

- **Untranslated messages from the layout engine and the kernel.** Warnings produced by `riku-mod-layout` and the core (for example "redes no comparadas en …" for a chip too large to compare, the `antes:`/`después:` prefixes on reader notices, the electrical summary in the layout viewer, some Git errors) are still in Spanish, whatever `RIKU_LANG` says. The CLI, the viewer UI and the module texts in `riku/locales/` are translated.
- **Letter spacing by font size** cannot be adjusted in egui. Check again with each new egui release.

### LVS

- **`log --lvs` and `status --lvs` use Netgen**, even though `riku lvs` defaults to the manual links in `lvs/<cell>.toml`. They need Netgen and the PDK's Netgen setup, and they report Netgen's verdict, not the status of the links.
- **The `inversor` demo ships no `lvs/*.toml`.** `riku lvs` on it starts with no links; use `riku lvs --suggest` or the Links tab in the viewer to create them.
- **The manual check covers transistors only.** Resistors, capacitors and subcircuits are listed as not checked.
- **The layout netlist for Netgen is flat.** There is no polygon cap, but Netgen takes about 20 minutes to compare the 1 KB SRAM macro of the `chip` demo.
- **OpenRAM's dual-port bitcell** (`sky130_fd_bd_sram__openram_dp_cell`) does not match Magic. It already differs when that single cell is extracted flat: Magic names `nfet_latch` what Riku names `special_nfet_pass`, and counts twice as many `pfet_latch`. It is the only difference when comparing the whole macro against Magic (`tools/verify/nets/chip_vs_magic.sh`).

### Schematics

- **Pin rewiring between existing nets is not reported.** The Xschem diff compares each component's parameters and symbol, and nets as a set of names; it does not compare the pin-to-net map. Moving a pin from one existing net to another, with no net appearing or disappearing, produces no change (`external/xschem-viewer-rust/src/semantic.rs`). Comparing connectivity per pin is the fix.

### Layouts

- **Whole chips in the diff.** A cell above 2 million flattened polygons has its nets compared only if its extraction is already in memory or on disk, or with `RIKU_FULL_NETS=1`; otherwise the diff warns and compares its sub-cells.
- **Large cells in the viewer.** Above 2 million flattened polygons, the viewer counts transistors and nets but does not draw them, and there is no net under the cursor.
- **Inlining small cells is a size rule.** Every sub-cell under 256 flattened polygons (`RIKU_HIER_INLINE`) is merged into its parent. With 128 the 1 KB macro extracts in 3.7 s instead of 6 s, but a connection in `precharge_0` of the SRAM example goes missing (not yet investigated).
- **Top-cell changes over an instance** are shown as belonging to the instance whose box contains them (the `met5` straps of the `chip` demo appear under `bank`). Nets already handle this correctly; the displayed origin does not.
- **Magic labels that are not ports** are anchored at a point on their edge, while Magic uses the whole rectangle. Ports already use the rectangle; other labels would need the Magic reader to pass it on.
- **Cell renames with changes.** Only pure renames are detected; a renamed cell that also changed shows as removed plus added.
- **Diodes and capacitors** are not extracted, and a resistor that changes is not listed in the diff (resistors are in the netlist).
- **Magic format coverage:** `.mag.gz`, `MASKHINTS_*` as geometry and `maglef/` views are not read.
- **Dense zoom.** Zoomed in on very dense areas, a frame takes about 45 ms, because pixels smaller than the finest level of the spatial pyramid draw every shape one by one.

### Project

- **Clippy does not block CI.** The Clippy job runs with `continue-on-error`; formatting (`cargo fmt --check`) does block.
- **No real-user testing yet.** The viewer has been tested with simulated clicks under Xvfb. It still needs designers using it, and the custom window frame (move, maximize, resize) needs testing on real desktops.

## Planned improvements

### Net extraction

| Item | Why |
|---|---|
| Early cutoff in the per-cell extraction | When a child changes inside but answers its parent's queries the same way, reuse the parent instead of rebuilding it. With the disk cache it would save about 2 s per commit on a whole chip. See [Cell summaries](design-notes.md#cell-summaries-d20) |
| Faster first extraction of a whole chip | The 1 KB macro takes 5–6 s the first time; the cost is in cells that inline hundreds of small sub-cells (`sense_amp_array`, `write_driver_array`). Needs profiling |
| Per-cell net probe in the viewer | Find the net under the cursor by descending through cells, without flattening, so large cells get nets and transistors drawn |
| Remove the flat path | `RIKU_FLAT_NETS=1` and the windowed flat extraction (`nets/context.rs`) remain only to compare against the per-cell extraction |
| Content-based inlining | Decide which sub-cells to merge into their parent by content (contacts, transistors without their well) instead of by size |

### LVS

| Item | Why |
|---|---|
| Hierarchical layout SPICE | One `.subckt` per cell, so Netgen compares a whole chip level by level in minutes instead of 20 |
| Cross-probing of nets | Click a net on one side to highlight its counterpart on the other, even when it matches. Transistors already do this in the Links tab through their links; nets, and the Netgen tab, do not (it would need Netgen's equivalence table) |
| Highlight only the mismatched L | When Netgen merges parallel devices of different L, highlight only the devices of the L that does not match |
| LVS of two versions in a diff | Open the LVS of both versions and see which discrepancies appeared or were fixed |
| LVS in the viewer's History panel | The same information as `riku log --lvs` |
| Pull request check | Base against head in CI, like `status --lvs` does for the working tree |
| LVS in `status` by default | A `.riku.toml` option to include the LVS in `riku status` without `--lvs` |
| Netgen in parallel | Run Netgen for several versions at once, after measuring |
| Lighter commit checkout | Write only the files a run reads, instead of the whole commit tree, when the cache misses |

### Diff, viewer and tooling

| Item | Why |
|---|---|
| Renames of cells that also changed | Match by bounding box and shared hashes (`gds_diff.rs::detect_renames`) |
| Finer spatial pyramid | Sparse bitsets for a finer level (`viewer-core/src/index.rs`) to speed up dense zoom |
| Measure the diff on a SKY130/Caravel wrapper | Performance was measured on an IHP chip; another PDK may have a worse case (`profile_diff`) |
| Cap the width of `log --graph` | Limit the number of lanes and mark the lanes that do not fit with `…` |
| Make Clippy blocking | Once the workspace is clean |
| More of the Magic format | `.mag.gz`, `MASKHINTS_*` as geometry, `maglef/` views |
| Diodes, capacitors and resistor changes | Extract diodes and capacitors, and list resistor changes in the diff |
| Terminal UI for the history | Optional: browse the graph with `ratatui`, without the viewer |

## Upstream (xschem-viewer-rust)

These depend on changes in the schematic library or in the contract it shares with Riku.

| Item | Why |
|---|---|
| Viewer memory peak | About 2.6 GB with a 42 MB chip. Lowering it means storing `DrawElement` points as `f32` or in CSR form, which changes the `viewer-core` contract |
| Per-process `.sym` cache in `RenderOptions` | Each schematic re-reads its symbols (about 1.6 ms each; seconds over a 1,000-commit `log`) |
| Cancellation between phases of `build_index` | Switching cells quickly leaves work running; it changes a signature that crate uses |

## Only if needed

Measure before doing any of these.

- **Recursive pieces when flattening** (top → core): about 1.2 GB estimated, never measured. Wait for a real design that shows it.
- **GPU mesh** for drawing layouts: only if a larger design drops below 60 fps.
- **Reading blobs by OID** in `log` (`old_oid`/`new_oid` in `ChangedFile`): little gain measured.
- **Cache of the waveform signal list:** only with 100,000+ signals.

## Ideas

Not planned; listed so they are not lost.

**Git integration**

- A `textconv` (`cachetextconv=true`) or difftool setup, so a plain `git diff` or `git log -p` shows the semantic diff (`*.sch diff=riku`).
- Merge drivers per format in `.gitattributes`, installed locally with `[include] path=.riku/gitconfig`, never globally:
  - `.mag`: normalize `timestamp` lines and run `git merge-file`;
  - `.sch`: merge disjoint components, and warn when a Move All is mixed with functional changes;
  - `.gds`/`.oas`: merge cells that are disjoint from the base, and report a conflict when both sides touch the same cell (careful with shared sub-cells and a top cell edited on both branches).
- Git LFS: Riku sees the pointer, not the file, so it cannot compare versions stored with LFS. Support it, or at least say so clearly. LFS is useful for large GDS files.
- Stale derived files after a merge (a `.gds` older than its `.mag`, a `.spice` older than its `.sch`): warn without blocking. Needs `.riku.toml` to declare what is source and what is derived.
- Commands: `blame --semantic` (who last touched a component or cell), `log --cell`/`--component`, and `log --sim-metric` (a measurement across history).

**Electrical**

- Electrical checks with external tools, and their difference between commits: ERC (unconnected gates or pins, unbiased wells), antenna (the rules in the PDK's DRC deck), parasitics (Magic `ext2spice` with `cthresh`/`rthresh`: "net `out` went up 12 fF") and post-layout (simulate the extracted netlist and compare the `.meas` results).
- Check whether KLayout's LVS is on par with Netgen.

**Verification in CI**

- DRC as a difference between base and head: `klayout -b -r script.drc` and read the `.lyrdb` (`ReportDatabase`). Fail only if violations increase, so existing ones are tolerated.
- `.meas` regression: read `name = value` from the ngspice log and compare with tolerances, against the parent or a nominal value. Only between runs of the same phase (pre- or post-layout).
- A pull request comment that updates itself, marked with `<!-- riku-ci -->`, and a `ci init` command with templates.
- Cache keys for verification results: tool version, PDK hash and, for LVS, the `setup.tcl`; never file dates. A shared cache (S3/R2) only for teams.

**Project and PDK**

- Pin the PDK per project in `.riku.toml` (`pdk.version`, a commit, like volare/ciel), checked by `riku doctor`. Also `layout.source = magic|klayout|python`.
- `riku doctor` could detect that SKY130 with KLayout needs two edits (`sky130.lym`, `sky130A.lyt`); without them it fails silently.
- New formats: SPICE/CDL netlists (canonicalize, or use Netgen's JSON; a `.spice` does not always derive from the `.sch`) and KiCad `.kicad_sch`.
- Origin of a flattened polygon: which instance and rotation it comes from ("instance of `amp` rotated 90°").
