# Design notes

The decisions behind how Riku extracts nets, names devices and runs LVS, written as short records so contributors know why the code is the way it is.

> [!NOTE]
> The full development history of these decisions, with the measurements and the alternatives that were dropped, is available at the `v0.2.0` tag under `docs/ronda-*` (in Spanish).

Each note has the same shape: **Context** (the problem), **Decision** (what the code does) and **Consequences** (what follows from it, including the limits). File and function names refer to the current tree.

On this page:

- [Hierarchical net extraction](#hierarchical-net-extraction)
  - [Cell summaries (D20)](#cell-summaries-d20)
  - [Memory and disk cache (D21)](#memory-and-disk-cache-d21)
  - [Per-cell comparison (D22)](#per-cell-comparison-d22)
  - [Viewer and LVS on top of the per-cell extraction](#viewer-and-lvs-on-top-of-the-per-cell-extraction)
  - [How the per-cell extraction is verified](#how-the-per-cell-extraction-is-verified)
- [Where net and device names come from](#where-net-and-device-names-come-from)
- [Device rules with `+types`](#device-rules-with-types)
- [LVS result cache (D10)](#lvs-result-cache-d10)
- [LVS in log and status (D11, D12)](#lvs-in-log-and-status-d11-d12)
- [LVS pairing, opt-in and results](#lvs-pairing-opt-in-and-results)
- [Manual LVS links as the default, Netgen optional](#manual-lvs-links-as-the-default-netgen-optional)

## Hierarchical net extraction

**Context.** Riku used to extract a cell by flattening everything below it and building its nets from scratch. That has two costs. The same leaf cell (a bitcell used 8,192 times) is re-extracted in every commit, on both sides of a diff and in every viewer session. And a whole chip did not fit: above 2 million flattened polygons (`devices::MAX_POLYGONS`) nets were not computed at all.

**Decision.** Nets and transistors are extracted **per cell**, from the leaves up, and each cell's result is stored by a content hash of everything that determines it. The hierarchy is treated as a DAG, not a tree: a cell used many times is one node, so the cost grows with the number of distinct cells, not with the number of instances. Independent cells are extracted in parallel with `rayon`.

The code lives in `riku-mod-layout/src/nets/hier/`:

| File | Role |
|---|---|
| `mod.rs` | Public API: `extract`, `flatten`, `HierNets`, `Pair` (both sides of a diff), `opened_cell_changes` (the viewer) |
| `build.rs` | A cell's summary from its own geometry and its children's summaries; inlining of small cells; substrate |
| `touch.rs` | Which pieces from different cells conduct together (same rule as the flat extractor) |
| `xf.rs` | Exact Manhattan instance transforms (no sine/cosine rounding) |
| `key.rs` | `NetKey`, the content hash of a cell's extraction |
| `memo.rs` | The in-process memory of summaries, own parts and neighbourhoods |
| `disk.rs` | The on-disk cache of summaries |
| `compare.rs` | Opens, shorts, renames and transistor changes of one cell between two versions |
| `check.rs` | `same_netlist`, the oracle that compares a flat and a per-cell netlist |

**Consequences.** A whole chip can be extracted (the 1 KB OpenRAM macro in the `chip` demo, 127,628 transistors, in seconds), and an unchanged cell is never extracted twice in a process or, if it was slow, across runs. The flat extractor (`nets::cell_nets`, `nets/context.rs`) is still in the tree behind `RIKU_FLAT_NETS=1`, used only to compare the two paths.

### Cell summaries (D20)

**Context.** A parent must be built from its children without looking inside them again, and the result must be the same netlist the flat extractor produces.

**Decision.** A cell's summary (`CellNets` in `build.rs`) holds:

- its own pieces and their nets, and its own transistors;
- its instances: the child's `NetKey` and the transform (one per repetition of an array reference);
- its pins: which child nets each own net joins.

The child's summary is not copied; it is looked up by its key. Building a parent looks only at its own geometry and at the child pieces that touch something of the parent or of a sibling. Child nets that nothing touches are not copied: they stay internal to the child, which is where the memory and time savings come from. Touching sibling pairs are memoized as neighbourhoods keyed by `(child key, child key, relative transform)`, so a mirrored array of thousands of bitcells computes a handful of distinct neighbourhoods and reuses them.

Instances with a magnification or a non-Manhattan angle are flattened into the parent. Cycles, and cells without a hierarchical hash, are extracted flat.

The original design flattened every instance whose layers interacted with its surroundings. Measured on the SRAM example, that marked 3,424 of 3,574 instances and saved nothing, so the implementation resolves context **like Magic does when it reads a GDS**: each cell is evaluated on its own, with three adjustments that cover what depends on the surroundings in practice:

1. **Small cells are inlined into their parent**, like Magic's `gds flatglob`. A contact cell or a lone transistor means nothing alone (a `licon` without the diffusion under it is no type). The threshold is 256 flattened polygons (`RIKU_HIER_INLINE`). Thresholds of 192 and above match the flat netlist on the SRAM example; 128 and below do not.
2. **Rule closures** (`grow` followed by `shrink` in the `.tech`, `DeviceRules::bridge`): two pieces of the same type closer than that distance are joined across cells. In SKY130 the P-well joins wells of different cells less than 0.84 µm apart.
3. **The substrate is resolved upward.** A child's P-well pieces stay as substrate candidates until a deep N-well above excludes them or the extracted root joins them to the substrate. Bodies that a child alone would place "in the substrate" go to whatever well the parent has at that point.

**Consequences.** On the SRAM example, the OTA, the inverter (Magic) and several parts of the 1 KB macro, the per-cell netlist is identical to the flat one. The inline threshold is a size heuristic, not a content rule; a lower one is faster but loses a connection on the SRAM example (see the [roadmap](roadmap.md)). Two things from the original design were not built: the **early cutoff** (reusing a parent when a changed child answers the parent's queries the same way) and stable re-numbering for it. Without them the design is still correct; a change inside a child rebuilds the child's ancestors, which is cheap because neighbourhoods and unchanged siblings come from memory.

### Memory and disk cache (D21)

**Context.** The same cell appears on both sides of a diff, in every commit of a `log`, and every time the viewer opens it or a parent of it. Recomputing it is the main cost.

**Decision.** `NetKey` (`key.rs`) is a 128-bit Merkle hash over the DAG, computed alongside the geometry hashes. A cell's key combines its own polygons, its labels (layer, text, quantized origin), its Magic ports (name and rectangle), and each reference as `(child NetKey, transform)`. A per-library salt adds what interprets the geometry: the device rules' fingerprint (a hash of the `.tech` or compiled table), the library unit, the layer names, the inline threshold and the `riku-mod-layout` version. A cell without a hash has no key and is never stored.

Results are kept at two levels:

| Level | Where | Limits |
|---|---|---|
| Process memory (`memo.rs`) | Summaries by `NetKey`, own parts by an own-content key, neighbourhoods | 512 MB estimated by default (`RIKU_NETS_MEM_MB`, `0` disables it); least recently used entries are evicted down to three quarters of the cap |
| Disk (`disk.rs`) | `<cache dir>/nets/<key>.json`, next to the diff cache | Only summaries that took at least 20 ms to build; 512 MB cap, oldest removed first; a format version invalidates old entries |

The disk cache follows the same switches as the diff cache: `RIKU_CACHE_DIR`, `RIKU_NO_CACHE` and `--no-cache`. A child is read from disk only when a query or a flatten needs it. When two threads need the same cell, one builds it and the other waits without blocking the `rayon` pool (after 5 s, or if it is the same thread, it builds it again rather than risk a deadlock).

**Consequences.** A key that covers the rules, unit and version means a new `.tech` or a new Riku release can never reuse a stale extraction. An unreadable disk entry is deleted and recomputed; it is never an error for the user. The comparison cache keyed by `(key A, key B)` from the original design was not built: comparisons are recomputed, while the extractions they rely on come from the cache.

### Per-cell comparison (D22)

**Context.** The net diff (opens, shorts, renames) used to flatten the changed cell, compare only near the change and then confirm against the whole cell. That was slow and did not work above the polygon cap.

**Decision.** `gds_diff` compares nets and transistors **per cell** (`compare.rs::cell_changes`, driven by `nets::hier::Pair`):

- Only cells with a geometry change on a layer the device rules use, and their ancestors, are compared. A pair of cells with the same `NetKey` is skipped.
- Anchors between the two versions are the cell's labels, its own transistors matched by position, and its instances: **twins** (same `NetKey` and transform) share their nets; an instance whose child changed is matched by name and transform, and its port nets by label.
- The classification is shared with the flat diff: several nets of A into one of B is a short; one into several is an open (or a split, if each part keeps a label of the short); one to one with another label is a rename.
- A short is reported only in the **lowest** cell where it appears, not again in each ancestor.
- A net with no label is named after the child it comes from: `<child>@(x, y)/<child net>`, with the instance origin in µm.

**Consequences.** Cells above 2 million flattened polygons (a whole chip) are compared per cell only if their extraction is already in memory or on disk, or with `RIKU_FULL_NETS=1`; otherwise the diff warns and compares their sub-cells. This keeps any case from becoming slower than before (the first extraction of a whole chip costs seconds). In the viewer, a cell below the cap is still compared flattened (with the per-cell extraction and its memory underneath); above the cap it is compared per cell.

### Viewer and LVS on top of the per-cell extraction

**Decision.** The viewer's electrical layer (`viewer_core_compat::add_electrical`) calls `hier::extract`. Below the polygon cap it flattens the result and draws transistors and nets as before. Above it, it only reports how many transistors and nets there are and asks the user to open a sub-cell. Because the result is memoized, reopening a cell, or a parent of it, does not extract again.

The layout netlist for LVS (`nets::layout_netlist`, `nets::layout_spice`) always flattens the per-cell extraction and writes a **flat** SPICE netlist, with the same names the viewer's net probe uses.

**Consequences.** LVS no longer has a polygon cap, but Netgen takes about 20 minutes to compare the 1 KB macro flat. Writing one `.subckt` per cell so Netgen can compare level by level was designed but not built, and Netgen output is read from its top-level entry only. The viewer's net probe above the cap (finding the net under the cursor by descending through cells) is also not built. Both are on the [roadmap](roadmap.md).

### How the per-cell extraction is verified

**Decision.** `same_netlist` (`check.rs`) compares a flat and a per-cell netlist of the same cell: transistors matched by model and position with equal W and L, and nets compared as a bijection derived from terminals and labels, with the same names, pins and substrate. It runs in `cargo test` and through the `hier_check` example. The scripts `tools/verify/nets/hier.sh` (standard cells and demo cells across three PDKs, plus Netgen on the flat versus flattened SPICE) and `tools/verify/nets/chip_vs_magic.sh` (the 1 KB macro against Magic's `extract all` + `ext2spice lvs`, compared with Netgen) cover what unit tests cannot.

**Consequences.** Against Magic, the whole macro matches except OpenRAM's dual-port bitcell, which already differs when that single cell is extracted flat (a known limitation, see the [roadmap](roadmap.md)). See [Verification](development.md#verification) for how to run the scripts.

## Where net and device names come from

**Context.** In the LVS view, Netgen reports layout names such as `19` (a transistor) or `Vout` (a net). The viewer has to turn those names into geometry. If the viewer and the LVS netlist named things differently, a click would highlight the wrong device.

**Decision.** The LVS netlist and the viewer's probe come from **the same `Netlist`**, built by the same function on the same cell, and the probe (`nets/probe.rs`, `LayoutNets`) uses the rules `spice()` uses to write names:

- **Nets** by `Netlist::net_name`: the label, `VSUBS` for the substrate, or `n<i>`. Two separate nets with the same label are written with one name, so Netgen sees one net; `net_named` returns the pieces of both. Lookup is exact first, then case-insensitive if that is unambiguous.
- **Transistors** by their index in `Netlist::devices`. `device_named` accepts `19`, `X19` and `M19` (and `R3`/`XR3` for resistors), because Netgen strips the `X` of subcircuit instances and open PDKs model transistors as subcircuits.
- **Parallel devices.** Netgen merges devices in parallel with the same model, gate, body and source/drain pair **even when L differs**, and names the group after its lowest index. The probe uses `nets::parallel_groups` (which ignores L), not `nets::fingers` (which groups by L too and gave one group more than Netgen on the `ota` demo).
- **Magic ports.** The viewer passes the Magic reader's `MagInfo` (which labels are ports) to the extraction, exactly like `riku lvs`. Without it, every label counts as a pin and a net can be named after a different label.

The schematic side maps names to geometry through `Report::places`, written by the same netlister Netgen saw, so it is exact even for unnamed nets.

**Consequences.** A unit test writes a netlist with `spice()` and checks that every device and net name in it resolves through the probe; changing how `spice()` names things breaks the test. When a name cannot be placed (a cell too large to compute nets, a layout without a known PDK), the list says so and only the schematic is highlighted. The manual LVS links do not use these indices; see [the last note](#manual-lvs-links-as-the-default-netgen-optional).

## Device rules with `+types`

**Context.** Magic's `.tech` file chooses a transistor model per `device` line, in order. Riku only understood W and L conditions, so on lines such as `device … +npn,pnp` it dropped the `+types` token. For SKY130's 5 V devices (`mvnfet`/`mvpfet`), the first line, which marks the part of a bipolar to ignore, matched every transistor, and the inverter demo did not match LVS.

**Decision.** `devices/rules.rs` parses each line into a `ModelRule` with its W/L conditions, its `near` types (from `+a,b`) and, when the terminals are not all the same type (drain-extended devices such as `g5v0d16v0`), the type required at each terminal. `DeviceType::model_for` picks the first rule whose W/L conditions hold and whose `near` types and terminal types are present at the gate (checked on the geometry during extraction). A rule that resolves to `Ignore` drops the transistor, as Magic does. `DeviceType::model`, used where no geometry is at hand, skips the rules that need it.

**Consequences.** SKY130's 5 V transistors come out with their model, and the standard cells of the three PDKs extract exactly as before. The compiled rule tables (`devices_generated.rs`) are a copy of the `.tech` text and are parsed the same way.

## LVS result cache (D10)

**Context.** A Netgen run takes seconds per pair and version, and `riku lvs --log`, `log --lvs` and `status --lvs` run it for many versions. An earlier cache keyed results by the Git ids of the pair's two folders. It missed symbols in other folders, sub-cells elsewhere and a new PDK or Netgen, so it could return a stale "match". For LVS, a false pass is the worst possible error.

**Decision.** Results are cached **by dependencies** (`riku/src/lvs/cache.rs`):

- Every project file the run reads goes through a `RecordingFiles` wrapper around the `FileSource` given to the netlister and the layout reader. It records each path with the Git blob id of its content, and also the paths that were looked up and **not found**.
- What does not go through that source is hashed into an environment fingerprint: Riku and netlister versions, the PDK (folder, `nodeinfo.json` when present, the Netgen `setup.tcl`, the `xschemrc` and the symbol search paths) and the Netgen executable.
- An entry is valid for a version (a commit or the working tree) only if the fingerprint matches and every recorded path has the same blob id there, with missing files still missing. For a commit the ids come from the tree without writing anything.
- Entries live in memory per process and on disk under `<cache dir>/lvs/v2/<pair>/`, at most 200 per pair (oldest removed). Errors are cached in memory only.

**Consequences.** A commit that touches nothing the run read (a testbench, for example) reuses the result in milliseconds; a symbol added anywhere on the search path invalidates it. On a working tree with `core.autocrlf`, a file's id may differ from its blob id; that only causes a recomputation, never a stale result. `RIKU_NO_CACHE` disables the disk cache.

## LVS in log and status (D11, D12)

**Context.** A verdict alone ("parameters differ (5)" in three consecutive commits) does not say what changed. Designers need the LVS next to the commit's other changes, and a check before committing.

**Decision.** `riku/src/lvs/annotate.rs` adds the LVS to existing reports:

- **`riku log --lvs`** computes each pair in each commit and in its first parent (usually the next commit in the list, already memoized), and attaches the verdict, the transition (`broke`, `worse`, `better`, `fixed`) and the delta: discrepancies that appeared, were fixed or changed. It never makes `log` fail: without Netgen, or without pairs, it adds a warning.
- **`riku status --lvs`** compares the working tree with `HEAD` per pair and sets the exit code from the LVS (`lvs_types::status_outcome`): `2` if any pair errors in the working tree (Netgen missing, broken netlist), else `1` if any pair broke or got worse, else `0`. A pair with the same verdict but new discrepancies exits `0` with a warning.
- Discrepancies have a stable key built from schematic names (`P:M3:w`, `N:Vout,Vp`, `D:M8`, `pin:Ib`), because layout names are indices that shift when a transistor is added. If one side's connections do not match, parameters are left out of the delta, since Netgen does not compare them in that case.
- The text output shows up to three delta lines by default, all with `--detail`, and the full discrepancy list with `--full`. The JSON adds an optional `lvs` field without changing the schema version.

**Consequences.** `riku status --lvs -f json > /dev/null` works as a pre-commit hook. Both flags run **Netgen**, even though `riku lvs` now defaults to manual links (see the [roadmap](roadmap.md)).

## LVS pairing, opt-in and results

These decisions were made together, when the LVS moved into the history; the comparator decision was later revised (see the next note).

1. **Pairing by name, with configuration taking priority.** `--sch`/`--layout` override `[[lvs]]` entries in `.riku.toml`, which override name matching: an Xschem schematic pairs with the layout of the same name, preferring `.gds`, then `.oas`, then `.mag`. The common case needs no configuration. When the name picks one of several layouts, Riku says which one it chose and suggests fixing it in `.riku.toml`.
2. **LVS in `log` and `status` only with `--lvs`.** A fresh run costs seconds per pair and version; the ordinary `log` must not wait for an external tool or fail when it is missing. With `--lvs`, the exit code of `status` becomes the LVS one. An option to turn it on by default in `.riku.toml` was deferred.
3. **Results are not versioned; they are recomputed with a cache.** A result is derived from the PDK and the comparator. Committing it would leave stale results and create a conflict on every merge. The [dependency cache](#lvs-result-cache-d10) makes recomputation cheap.
4. **Netgen as the comparator, rather than a graph matcher of Riku's own.** Netgen is the reference comparator for SKY130, GF180MCU and IHP, and its per-PDK rules (parallel devices across L, properties, dummy devices) were already needed by the LVS view. This still holds for automatic comparison (`--netgen`, `log --lvs`, `status --lvs`), but it is no longer the default for `riku lvs`.

## Manual LVS links as the default, Netgen optional

**Context.** Netgen answers "do these match?" as a whole, with layout names that are indices into the extracted netlist. It must be installed with the PDK's setup, and its result is all-or-nothing: it gives no help while a layout is half drawn, and its layout names change whenever a device is added. What designers wanted to track over time was which schematic transistor *is* which layout transistor, and what follows from that.

**Decision.** `riku lvs` and the viewer's LVS view use **manual links** by default (`riku/src/lvs/manual/`); Netgen runs only with `riku lvs --netgen` or from the Netgen tab in the viewer.

- **Links are versioned with the design** in `lvs/<cell>.toml` (schema `riku-lvs-map/v1`). Each link is a schematic transistor and the layout fingers that form it, written in natural order (`M2` before `M10`) so a commit's diff reads well. The file is written atomically (temporary file, then rename).
- **A layout transistor is identified by its model and the center of its gate**, in µm in the compared cell's coordinates (within 0.01 µm), plus its sub-cell and local position when it is drawn in one. That identity survives renumbering, and survives the cell being moved within a chip.
- **Relocation, only when unambiguous.** If links no longer land on a gate, Riku tries a rigid motion (eight orientations, RANSAC-style: a hypothesis is discarded with two witnesses, and survivors are counted), then the move of a sub-cell instance, then connectivity: the only free transistor of the right model connected to nets already linked. A rigid motion that renames named nets (a mirrored differential pair swapping `Vp` and `Vn`) is penalized. If more than one alignment fits, nothing is relocated: better unlinked than wrongly linked. `--update` writes the new positions back.
- **Suggestions without guessing** (`--suggest`): starting from nets with the same name on both sides (the pins) and nets already linked, a schematic transistor is linked only if exactly one group of fingers fits. Symmetric devices are left for the user to pick.
- **What is checked:** model and parameters of each linked pair, shorts and opens deduced when links contradict each other's nets, pins on both sides, and how many transistors are still unlinked. Resistors, capacitors and subcircuits are listed as not checked. "Clean" means everything is linked with no differences.
- **Inputs are the SPICE netlists of both sides**, the same Netgen would read: it is the one format where model, W and L are already resolved (after the symbol's `format` is expanded). The schematic is flattened with instance paths (`x1/M3`), and layout transistors keep their sub-cell, so the check is hierarchical.
- **History and cache.** `riku lvs --log` shows how the links' status changes per commit, marking where it stopped being clean. A commit without a links file uses the one on disk, so today's links can check older commits. Extractions are cached by dependencies like the Netgen results (`<lvs cache>/manual-v1/<pair>`), and a commit is validated by reading from Git without writing the tree out.
- **In the viewer**, the Links tab is the default. Each transistor is colored by its status, and clicking one on each side (Shift adds fingers) links them and updates the file.

**Consequences.** `riku lvs` works with no external tool, its result is stable across commits, and progress is visible on a partial layout. It is not an automatic comparator: someone, or `--suggest`, has to create the links, and the `inversor` demo ships without a links file. `riku log --lvs` and `riku status --lvs` still use Netgen. The schematic netlister is shared with the Netgen path rather than adding a second structured representation that could diverge. See [the LVS guide](../lvs.md) for the workflow.
