# Scripting, CI and agents

How to use Riku without a person in front of it: JSON output and its schemas, exit codes, CI recipes, and what scripts and AI agents should know.

On this page:

- [The contract in short](#the-contract-in-short)
- [Schemas](#schemas)
- [The change model](#the-change-model)
- [`riku-diff/v2` and `riku-diff-set/v1`](#riku-diffv2-and-riku-diff-setv1)
- [`riku-show/v1`](#riku-showv1)
- [`riku-status/v2`](#riku-statusv2)
- [`riku-log/v2`](#riku-logv2)
- [`riku-doctor/v1`](#riku-doctorv1)
- [LVS schemas](#lvs-schemas)
- [Errors as JSON](#errors-as-json)
- [Exit codes](#exit-codes)
- [Images for agents](#images-for-agents)
- [Non-interactive behavior](#non-interactive-behavior)
- [CI recipes](#ci-recipes)

## The contract in short

- `status`, `diff`, `show`, `log`, `lvs` and `doctor` accept `-f json`. Every JSON document has a `schema` field such as `"riku-diff/v2"`.
- The data goes to **stdout**. Warnings and progress notes (for example `LVS: 1 new Netgen run(s)`) go to **stderr**, so `riku … -f json > out.json` gives you clean JSON.
- With `-f json`, an error is also JSON on stdout (`riku-error/v1`).
- The **exit code** tells you whether there were functional changes (`status` always; `diff` and `show` with `--ci`) or whether the LVS is clean (`lvs --ci`, `status --lvs`).
- `diff`, `show`, `status` and `log` print indented JSON by default; `--compact` prints it on one line. `lvs` and `doctor` always print indented JSON.
- Key order inside objects is not significant (Riku prints keys in alphabetical order in some documents and in a fixed order in others).
- `.riku.toml` at the repository root fixes the comparison options for everyone, so a script gets the same answer as a person without repeating flags. See [configuration.md](configuration.md#project-file-rikutoml).

```bash
riku status -f json            # which files changed (exit code 1 if any change is functional)
riku diff -f json              # the detail, working tree against HEAD
riku diff amp.sch -f png       # and what it looks like
```

## Schemas

| Schema | Produced by |
|---|---|
| `riku-diff/v2` | `riku diff … FILE -f json` (one file) |
| `riku-diff-set/v1` | `riku diff -f json` without a file |
| `riku-show/v1` | `riku show COMMIT [FILE] -f json` |
| `riku-status/v2` | `riku status -f json` |
| `riku-log/v2` | `riku log -f json` |
| `riku-doctor/v1` | `riku doctor -f json` |
| `riku-error/v1` | Any of the above when the command fails |
| `riku-lvs-check/v1` | `riku lvs -f json` (manual links) |
| `riku-lvs-map-log/v1` | `riku lvs --log -f json` |
| `riku-lvs/v1` | `riku lvs --netgen -f json` |
| `riku-lvs-log/v1` | `riku lvs --netgen --log -f json` |
| `riku-lvs-map/v1` | Not an output: the format of the `lvs/<cell>.toml` links file (see [lvs.md](lvs.md)) |

**Compatibility policy.** An incompatible change bumps the version suffix (`v2` → `v3`). New optional fields, new element types, new detail keys and new enum values do **not** bump it. So a consumer should:

- check `schema` and refuse versions it does not know;
- ignore fields it does not know;
- treat an unknown `element.type` as an opaque change (count it, show its `kind`, skip type-specific handling);
- read optional fields defensively: several are omitted when empty (listed below).

## The change model

`diff`, `show`, and the `--full` level of `status` and `log` describe changes with the same typed objects, whatever the format.

### A file

| Field | Type | Meaning |
|---|---|---|
| `format` | string | `xschem`, `gds` (GDSII, OASIS and Magic), `waveform` (ngspice `.raw`) or `unknown` |
| `changes` | array | The changes (below). Empty means "no changes" **only if `error` is null** |
| `error` | string or null | The module could not compare the file (one side damaged or unreadable, or over 50 MB). Then `changes` is empty and says nothing |
| `warnings` | array of strings | Non-fatal notes (a missing Magic sub-cell, an axis that changed…). The rest was compared |

### A change

| Field | Always | Meaning |
|---|---|---|
| `kind` | yes | `added`, `removed`, `modified` or `renamed` |
| `element` | yes | What changed, typed by `element.type` (below) |
| `cosmetic` | yes | `true` if it does not alter function (a move, an area under the threshold, a waveform within tolerance) |
| `severity` | no | `"error"` for a layout open or short: it changes the circuit. Absent for ordinary changes |
| `renamed_from` | no | The previous name, when `kind` is `renamed` |
| `location` | no | `{min_x, min_y, max_x, max_y}` in the format's units (µm in layouts) |
| `details` | no | Properties before and after: `[{key, before?, after?, placement?}]` |
| `position_changed` | no | `true` if, besides the change, the element moved (position, rotation or mirror) |

In `details`, `before` and `after` are typed values (numbers stay numbers, not text) and either can be missing when the property appears or disappears. `placement: true` marks a detail that is about where the element is drawn, not a parameter.

### Element types

| `type` | Fields | Used by | Typical `details` keys |
|---|---|---|---|
| `component` | `name` | Schematics: an instance (`M3`, `R1`, `x1`) | Its properties (`W`, `value`, `symbol`…); `inside` when only a sub-schematic changed |
| `net` | `name` | Schematics | — |
| `whole` | — | The whole file (for example a schematic where everything moved) | — |
| `cell` | `name` | Layouts: a cell added, removed or renamed | — |
| `geometry` | `cell`, `layer`, `datatype`, `layer_name`?, `via`? | Layouts: the shapes of one layer in one cell | `added_polygons`, `removed_polygons`, `added_area_um2`, `removed_area_um2` |
| `port` | `cell`, `name` | Magic layouts: a port | `class`, `use` |
| `device` | `cell`, `model`, `at` | Layouts: a transistor, `at` is a point inside its gate, in µm | `model`, `w_um`, `l_um` |
| `layout_net` | `cell`, `name` | Layouts: a net (its label, or a description of where it is if it has none) | `kind` (`open`, `short`, `separated`, `renamed`) and `nets` (the nets before and after, comma-separated) |
| `signal` | `plot`, `name` | Simulation results: a signal in one analysis | `plot`, `unit`, `max_abs_diff`, `at`, `x_unit`, `rms_diff`, `rel_diff`; `expression` and `value` for computed signals |

- `layer_name` is present when the file names its layers (Magic, or an OASIS file with layer names).
- `via` is present when the change comes from an instantiated sub-cell: `{path, instances, at?}`, where `path` is the list of cells from the root (excluding it), `instances` how many instances this change groups, and `at` the instance position when it is a single one.
- Opens and shorts (`layout_net` with `kind` `open` or `short`) carry `"severity": "error"`. A `separated` net (a short that was fixed) does not.

What each format detects is explained in [formats.md](formats.md).

## `riku-diff/v2` and `riku-diff-set/v1`

`riku diff A B FILE -f json` prints one file:

| Field | Meaning |
|---|---|
| `schema` | `"riku-diff/v2"` |
| `file` | The path, from the repository root |
| `from`, `to` | The two versions as given (`HEAD`, a hash, a tag) or `worktree` for the disk |
| `format`, `error`, `warnings`, `changes` | As in [A file](#a-file) |

Without a file, `riku diff -f json` prints `riku-diff-set/v1`: `schema`, `from`, `to` and `files`, where each entry of `files` has the same shape as a file of `riku-show/v1` (below).

## `riku-show/v1`

A commit and, per file, the typed changes. This is the commit of the `ota` demo that introduced a short:

```bash
riku show HEAD~3 -f json
```

```json
{
  "commit": {
    "author": "Riku Demo",
    "message": "Layout: route Vout to the left edge",
    "oid": "120ee0b98c2c799ce58c2ada65aec0e3fca4efdd",
    "parents": [
      "ad104a7ff5bea8ea01f62c2921e23851f16fd706"
    ],
    "short_id": "120ee0b",
    "timestamp": 1772818200
  },
  "files": [
    {
      "changes": [
        {
          "cosmetic": false,
          "details": [
            {
              "after": "short",
              "key": "kind"
            },
            {
              "after": "Vout = Vp",
              "before": "Vout, Vp",
              "key": "nets"
            }
          ],
          "element": {
            "cell": "ota-5t",
            "name": "Vout = Vp",
            "type": "layout_net"
          },
          "kind": "modified",
          "location": {
            "max_x": -0.4,
            "max_y": 9.55,
            "min_x": -5.6000000000000005,
            "min_y": -0.56
          },
          "severity": "error"
        },
        {
          "cosmetic": false,
          "details": [
            {
              "after": 1,
              "key": "added_polygons"
            },
            …
          ],
          "element": {
            "cell": "ota-5t",
            "datatype": 20,
            "layer": 70,
            "type": "geometry"
          },
          "kind": "added",
          …
        }
      ],
      "error": null,
      "file": "layout/ota-5t.gds",
      "format": "gds",
      "old_path": null,
      "status": "modified",
      "warnings": []
    }
  ],
  "schema": "riku-show/v1"
}
```

| Field | Meaning |
|---|---|
| `commit` | `oid`, `short_id`, `author`, `timestamp` (Unix seconds), `message`, `parents` (empty for the initial commit) |
| `files[].file` | Path in the commit |
| `files[].status` | `added`, `removed`, `modified` or `renamed` |
| `files[].old_path` | The previous path of a renamed file, otherwise null |
| `files[].format`, `error`, `warnings`, `changes` | As in [A file](#a-file). For a file without a module, `format` is null and `changes` is empty |

## `riku-status/v2`

```json
{
  "schema": "riku-status/v2",
  "branch": {
    "name": "scratch",
    "head_oid": "421f72d6f489274ee4754f82aa971708831b2f28",
    "head_short": "421f72d",
    "upstream": null,
    "ahead": 0,
    "behind": 0
  },
  "files": [
    {
      "path": "lvs/ota-5t.toml",
      "format": "unknown",
      "category": "unknown",
      "counts": {}
    },
    {
      "path": "xschem/ota-5t.sch",
      "format": "xschem",
      "category": "semantic",
      "counts": {
        "components_modified": 2
      }
    }
  ],
  "warnings": []
}
```

| Field | Meaning |
|---|---|
| `branch` | `name`, `head_oid`, `head_short`, `upstream` (or null), `ahead`, `behind`. Null in a repository without commits |
| `files[]` | One per modified file (all of them, including `unknown`, regardless of `--include-unknown`) |
| `files[].category` | `semantic`, `cosmetic`, `unchanged`, `unknown` or `error` (see [cli.md](cli.md#riku-status)) |
| `files[].counts` | Counters. Known keys: `components_added`, `components_removed`, `components_modified`, `components_renamed`, `nets_added`, `nets_removed`, `nets_modified`, `signals_added`, `signals_removed`, `signals_modified`, `shorts`, `opens`. Other keys can appear |
| `files[].errors` | Present when `category` is `error`: why it could not be compared |
| `files[].warnings` | Present when the module reported warnings |
| `files[].details` | With `--detail` or `--full`: one entry per change (below) |
| `files[].full_report` | With `--full`: the module's full result, shaped like [A file](#a-file) |
| `warnings` | Non-fatal notes for the whole run |
| `lvs` | With `--lvs`: one entry per pair (below) |

A `details` entry is `{kind, element, renamed_from?, params?}`. `kind` is one of `component_added`, `component_removed`, `component_modified`, `component_renamed`, `net_added`, `net_removed`, `net_modified`, `signal_added`, `signal_removed`, `signal_modified` or `other`; `element` is typed as in [Element types](#element-types); `params` maps each changed parameter to `"before → after"` (location changes are left out):

```json
"details": [
  { "kind": "component_modified", "element": { "type": "component", "name": "M1" }, "params": { "W": "4 → 6" } }
]
```

Each entry of `lvs` (`status --lvs`) is:

| Field | Meaning |
|---|---|
| `schematic`, `layout`, `cell`? | The pair |
| `head`, `worktree` | The LVS state on each side: `{"state": "done", "verdict": …}`, `{"state": "missing"}` (a file is not in that version) or `{"state": "error", "error": …}` |
| `unchanged` | `true` if the result is identical on both sides |
| `transition` | Present when the verdict changed: `broke`, `worse`, `better` or `fixed` |
| `delta` | `{appeared, fixed, changed}`: the discrepancies that appeared, were fixed, or changed value (`changed` holds `{before, now}`) |
| `discrepancies` | With `--full`: every discrepancy in the working tree |

`verdict` is `match`, `property_errors` or `mismatch`. A discrepancy is tagged by `kind`: `property` (`instance`, `layout_instance`, `model`, `param`, `schematic`, `layout`), `nets` or `devices` (`schematic` and `layout` name lists), or `pin` (`name`, `only_in`: `schematic` or `layout`).

## `riku-log/v2`

| Field | Meaning |
|---|---|
| `schema` | `"riku-log/v2"` |
| `commits[]` | Newest first (topological order with `--graph`) |
| `commits[].oid`, `short_id`, `author`, `timestamp`, `message` | The commit |
| `commits[].parents` | Parent OIDs (empty for the root commit, two or more for a merge) |
| `commits[].refs` | Branches, tags and `HEAD` pointing at it (omitted when empty) |
| `commits[].is_merge` | `true` for a merge, which has no per-file summary |
| `commits[].files` | Per-file summaries, with the same fields as `files[]` in `riku-status/v2` (omitted when empty) |
| `commits[].graph` | With `--graph`: the commit's place in the graph (below) |
| `commits[].lvs` | With `--lvs`: one entry per pair (below) |
| `warnings` | Non-fatal notes, such as `LVS not available: …` |

`graph` has `column` (0 is the left edge), `lane` (a stable identity of the visual branch, for coloring), `passing` (other branches crossing this row, as `[column, lane]`), `edges` (segments to the next row, as `[column here, column in the next row, lane]`) and `truncated` (the commit has parents beyond `-n`).

Each entry of `lvs` has `schematic`, `layout`, `cell`?, the state flattened in (`"state": "done"` with `verdict`, `"state": "missing"`, or `"state": "error"` with `error`), and, against the first parent, `transition`? and `delta`?, plus `discrepancies` with `--full`. The values are the same as in `riku-status/v2`.

## `riku-doctor/v1`

Tells a script what this `riku` can do before it tries:

```json
{
  "config": null,
  "lvs": {
    "netgen": "…/tools/bin/netgen"
  },
  "modules": [
    {
      "available": true,
      "extensions": [
        ".sch"
      ],
      "format": "xschem",
      "name": "xschem",
      "version": "Native renderer | PDK: gf180mcuD [ok]"
    },
    {
      "available": true,
      "extensions": [
        ".gds",
        ".oas",
        ".mag"
      ],
      "format": "gds",
      "name": "layout",
      "version": "riku-mod-layout (gdstk cxx; Magic in Rust)"
    },
    {
      "available": true,
      "extensions": [
        ".raw"
      ],
      "format": "waveform",
      "name": "spice",
      "version": "waveforms (ngspice .raw)"
    }
  ],
  "pdk": {
    "path": "…/pdks/gf180mcuD/libs.tech/xschem",
    "state": "found"
  },
  "repo": "~/riku-demos/ota/",
  "schema": "riku-doctor/v1",
  "symbols": true,
  "tools": {
    "path": "…/tools/xschem/share/xschem/xschem_library/devices",
    "state": "found"
  },
  "version": "0.2.0",
  "xschemrc": null
}
```

| Field | Meaning |
|---|---|
| `version` | The `riku` version |
| `repo` | The repository root, or null |
| `config` | Null without `.riku.toml`; otherwise `{path, error}`, where `error` is null if the file parses |
| `xschemrc` | The `.xschemrc` found (current folder, then home), or null |
| `pdk` | `{state, path}` for `$PDK_ROOT/$PDK`: `found`, `missing` (set but the path does not exist) or `not_configured` |
| `tools` | `{state, path}` for `$TOOLS`, with the same states |
| `symbols` | `true` if there is at least one source of Xschem symbols (`.xschemrc`, `$PDK_ROOT/$PDK` or `$TOOLS`) |
| `modules[]` | Each format module: `name`, `format`, `extensions`, `available`, `version` |
| `lvs` | `{"netgen": path or null}`, or null in a build without LVS |

`pdk.state` is `not_configured` when `$PDK` is unset even if Riku can still pick a PDK by the symbols of each schematic (the text output says so). See [configuration.md](configuration.md#pdk-and-symbol-discovery).

## LVS schemas

All four LVS outputs are always indented, and errors per pair do not stop the others: a failed pair appears as `{"schematic", "layout", "error"}` in the same list.

**`riku-lvs-check/v1`** (`riku lvs -f json`): `schema`, `version` (the revision or `worktree`) and `results[]`, one per pair. Each result has `schematic`, `layout`, `cell`, `map` (the links file), `map_from_disk` (the version had no links file and the disk's were used), `cached`, `clean`, `complete` (clean and nothing left unchecked), `schematic_devices`, `layout_devices`, `bound[]` (`{schematic, layout: [{model, at}]}`), `nets` (schematic net → layout nets), `params[]` and `models[]` (`{device, what}`), `shorts[]` and `opens[]` (`{net, nets}`), `pins[]` (`{device, what}` with the pin name in `device`), `lost[]`, `unknown[]`, `unbound_schematic[]`, `unbound_layout[]`, `unchecked[]`, `moved` (null or `{angle, mirrored, dx, dy, count}`), `moved_ambiguous`, `by_cell[]`, `by_connectivity[]` and `warnings[]`.

**`riku-lvs-map-log/v1`** (`riku lvs --log -f json`): `schema`, `from` and `results[]`, one per pair: `{schematic, layout, steps[]}`. Each step is `{commit, time, summary, state, transitions}`, or `{commit, time, summary, error}`. `state` holds the counts `bound`, `total`, `fingers`, `fingers_total`, `differences`, `shorts`, `opens`, `lost`, `pins`, `unchecked` and `clean`. `transitions` lists what changed against the older commit: `"clean"`, `"broke"`, `"new_short"`, `"short_fixed"`, `"new_open"`, `"open_fixed"` or `{"linked": N}`.

**`riku-lvs/v1`** (`riku lvs --netgen -f json`): `schema`, `version` and `results[]`. Each result has `schematic`, `layout`, `cell`?, `layout_cell`, `pdk`, `result` (`match`, `property_errors` or `mismatch`), `summary` (Netgen's final lines), `devices` and `nets` (`{schematic, layout}` counts), `pins`, `unmatched_nets[]` and `unmatched_devices[]` (`{schematic, layout}` name lists), `properties[]` (`{model, schematic, layout, values: [{name, schematic, layout}]}`) and `warnings[]`.

**`riku-lvs-log/v1`** (`riku lvs --netgen --log -f json`): `schema`, `from` and `pairs[]`: `{schematic, layout, cell, commits[]}`. Each commit is `{commit, author, time, state, …}`, newest first: with `"state": "done"` it carries the fields of a `riku-lvs/v1` result plus `reused`; with `"state": "error"`, `error`; with `"state": "missing"`, nothing more. `transition` and `delta` (as in `riku-status/v2`) are present when something changed against the older commit.

**`riku-lvs-map/v1`** is the `schema` line of the `lvs/<cell>.toml` links file; its format is documented in [lvs.md](lvs.md).

## Errors as JSON

When a command run with `-f json` fails as a whole (an unknown commit, no repository, an invalid `.riku.toml`), it prints one line on stdout and exits with an error code:

```json
{"schema":"riku-error/v1","error":"commit not found: v9"}
```

A file that could not be compared is not a command failure: it appears in the normal document with its `error` field set (or `category: "error"` in `status`), and the exit code reports it (below).

## Exit codes

| Command | 0 | 1 | 2 |
|---|---|---|---|
| `riku status` | No changes, or cosmetic only | Functional changes | Error, or a file that could not be compared |
| `riku status --lvs` | LVS same or better | A pair broke or got worse | LVS error (for example Netgen not installed) |
| `riku diff --ci`, `riku show --ci` | No changes, or cosmetic only | Functional changes | Error, or a file that could not be compared |
| `riku diff`, `riku show` | Success, whatever changed | Error, or a file that could not be compared | — |
| `riku diff`/`show` with `-f png`, `svg` or `visual` | Image written or viewer launched | Error (2 with `--ci`) | — |
| `riku lvs --ci` | Clean | Pending items or differences | Error |
| `riku lvs` (without `--ci`) | The check ran | Error | — |
| `riku log`, `doctor`, `render`, `demo`, `completions`, `open`, `gui` | Success | Error | — |

- `status` always uses the CI codes; its `--ci` flag is accepted and has no effect.
- With `--lvs`, the code of `status` is the LVS one and replaces the file-based one.
- The exact `lvs --ci` rules per mode (`--netgen`, `--log`) are in [cli.md](cli.md#riku-lvs).
- `riku log --lvs` never fails because of the LVS: problems become warnings.

## Images for agents

An agent that can read images can look at a change instead of parsing it:

```bash
riku diff HEAD~1 HEAD xschem/ota-5t.sch -f png       # prints the PNG path
riku show 120ee0b layout/ota-5t.gds -f png --cell ota-5t
riku render sim/ota-5t_tb.raw --rev v0.1 -o tb.png   # one version, no diff
```

The path of the image is the only line on stdout. No display or GPU is needed. Options and naming are in [cli.md](cli.md#images-riku-render-and--f-pngsvg). Because the exit code of an image is 0 whenever the image is written, pair it with `-f json --ci` when you also need to know whether the change is functional.

## Non-interactive behavior

- `riku` with no command opens the shell only when stdin is a terminal; otherwise it prints the help and exits with 0, so it never waits for input.
- `-f visual`, `riku open` and `riku gui` open windows: do not use them in automation. On a Linux machine without `DISPLAY` or `WAYLAND_DISPLAY` they fail with a clear message.
- Colors: the `log --graph` output is colored only on a terminal. `NO_COLOR`, `CLICOLOR_FORCE` and `--color` control it; JSON never has color.
- Truncating the output (`riku log | head`) ends quietly, as with other Unix tools.
- Messages and help follow `RIKU_LANG` (English by default). JSON keys and enum values never change with the language, but free-text fields (`error`, `warnings`, `what` in LVS results) do. Set `RIKU_LANG=en` if a script matches on text.
- Besides the images you send somewhere with `-o`, the only commands that write into your repository are `riku lvs --suggest` and `riku lvs --update` (the `lvs/<cell>.toml` file). Caches live outside the repository (see [configuration.md](configuration.md#caches)).

## CI recipes

### GitHub Actions: report what each commit changes

```yaml
name: riku
on: [push, pull_request]

jobs:
  semantic-diff:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0          # riku needs the parent commit to compare against

      - name: Install riku
        run: |
          curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"

      - name: Semantic changes of this commit
        run: |
          riku show HEAD -f json > riku-show.json || true
          riku show HEAD

      - name: Warn when the circuit changes
        run: riku show HEAD --ci > /dev/null || echo "::warning::this commit changes the circuit"

      - uses: actions/upload-artifact@v4
        with:
          name: riku-show
          path: riku-show.json
```

- `get.sh` installs the latest release into `~/.local/bin` and verifies its checksum; pass a version to pin it (`sh -s -- v0.2.0`).
- For a pull request, compare the branch against its base instead: `riku diff origin/${{ github.base_ref }} HEAD -f json --ci`.
- To fail the job on functional changes, drop the `|| echo …`: exit code 1 fails the step. Exit code 2 means Riku could not compare something and should not be ignored.
- Schematic symbols come from the PDK. If your schematics use PDK symbols, make the PDK available in the job and set `PDK_ROOT` and `PDK` (see [configuration.md](configuration.md#pdk-and-symbol-discovery)); `riku doctor` shows what was found.

### Pre-commit hook: block commits that break the LVS

`.git/hooks/pre-commit` (make it executable with `chmod +x`):

```bash
#!/bin/sh
# Refuse the commit if a schematic/layout pair stops matching or gets worse.
riku status --lvs > /dev/null
```

With `--lvs`, `riku status` exits with 1 when a pair broke or got worse and with 2 on an LVS error, which makes Git abort the commit; the details are on stdout if you run `riku status --lvs` yourself. It uses Netgen and caches results, so only changed pairs are recomputed. If Netgen is not installed every commit is refused (exit code 2), so only install this hook where Netgen is available. `git commit --no-verify` skips the hook.

To gate on the manual links instead (no Netgen needed), use the manual LVS in CI mode:

```bash
#!/bin/sh
riku lvs --ci > /dev/null
```

This refuses the commit while any pair is not fully clean (pending links, differences, or elements the manual LVS does not check), so it suits projects whose links are complete.

### Shell: list the files with functional changes

```bash
riku status -f json | jq -r '.files[] | select(.category == "semantic") | .path'
```

### Shell: find opens and shorts in a commit

```bash
riku show HEAD -f json \
  | jq -r '.files[] | .file as $f | .changes[] | select(.severity == "error") | "\($f): \(.element.name)"'
```
