# The viewer

Riku includes a desktop viewer for schematics (`.sch`, `.sym`), layouts (`.gds`, `.oas`, `.mag`) and simulation results (`.raw`) that shows the visual diff between any two versions, the repository history and the layout-versus-schematic links.

The viewer is read-only for your design files: the only file it writes is the LVS links file (`lvs/<cell>.toml`, see [LVS view](#lvs-view)). The interface is in English by default; **Settings → Language** (or `RIKU_LANG=es`) switches it to Spanish.

On this page:

- [Launching](#launching)
- [Home screen](#home-screen)
- [Repository-wide diff](#repository-wide-diff)
- [Viewing a diff](#viewing-a-diff)
- [Controls](#controls)
- [Transistors and nets](#transistors-and-nets)
- [Waveforms](#waveforms)
- [History panel](#history-panel)
- [Export and reload](#export-and-reload)
- [LVS view](#lvs-view)
- [Settings](#settings)
- [Window and preferences](#window-and-preferences)

## Launching

```bash
riku open                                  # home screen for the current folder; the terminal stays free
riku open amp.sch                          # open one file (or a folder)
riku gui                                   # same viewer, in this process (the terminal waits until you close it)
riku gui chip.gds --cell INV               # open a layout at a given cell
riku diff HEAD~3 HEAD -f visual            # the list of every file that changed between two versions
riku diff HEAD~1 HEAD chip.gds -f visual   # the diff of one file
riku diff HEAD amp.sch -f visual           # HEAD against the file on disk
riku show a3f2b1c -f visual                # everything a commit changed
```

| Command | Behavior |
|---|---|
| `riku open [FILE\|FOLDER]` | Starts the viewer as a background process and returns immediately. |
| `riku gui [FILE\|FOLDER] [--cell CELL]` | Runs the viewer in the current process. `--cell` opens a layout at that cell. |
| `riku diff … -f visual`, `riku show … -f visual` | Opens the viewer on a diff: with a file, that file's diff; without one, the [repository-wide list](#repository-wide-diff). |

Opening a file makes its folder the project folder; opening a folder makes it the project folder. The project's Git repository is detected from there.

**Requirements.** The viewer needs a graphical desktop: `DISPLAY` (X11) or `WAYLAND_DISPLAY` (Wayland) must be set. Without one, Riku says so and exits; every command-line feature keeps working. The installer also adds a **Riku** entry to your desktop's application menu, which runs `riku gui`.

## Home screen

The home screen is what you see with nothing open, and what the **Home** button in the top bar brings back.

| Card | What it shows | CLI equivalent |
|---|---|---|
| Project | The folder name and path, **Open folder…**, a line like `Git · main · 3 files with uncommitted changes`, and **Recent:** folders | `riku gui FOLDER` |
| Actions | **History** (also **H**), **Compare versions…** (one file or the whole repository), **Diagnostics** | `riku log --graph`, `riku diff A B [FILE] -f visual`, `riku doctor` |
| Uncommitted changes | Each changed file with its summary; a click opens its diff against `HEAD`. **↻** checks again | `riku status`, `riku diff FILE -f visual` |
| Recent | Files you opened recently | — |

History and Compare need a Git repository; without one the card says so and those buttons are disabled. Uncommitted changes are computed in the background, so the home screen appears immediately.

### Opening a folder

**Open folder…** (on the home screen and in the **Project** panel) opens Riku's own folder picker. Navigate through folders (folders that are Git repositories are marked) or type or paste a path and press Enter. **Show hidden** lists hidden folders.

When Riku runs inside a Linux container or WSL on a Windows machine, the picker also accepts Windows paths, either with backslashes or quoted the way File Explorer copies them. Riku translates the path to the folder where that Windows drive is mounted. Shortcuts to the Windows folders that are visible appear under **On the laptop:**, and the path bar shows what the current folder is called in Windows (`In Windows: …`). A Windows folder that is not shared with the container cannot be opened: the picker says so and lists the folders that are shared.

Switching folders reloads the project tree and points the History panel at the new repository.

### Opening files

- Click a file in the **Project** panel (left). Tick **All files** to list every file, not only the ones Riku can open.
- Drag a file from your file manager onto the window.
- Use **Recent** on the home screen.

With a file open, the top bar offers **Compare…** (with that file already selected) and **Export** (see [Export and reload](#export-and-reload)).

## Repository-wide diff

**Compare versions… → All files that changed**, `riku diff A B -f visual` without a file, or `riku show COMMIT -f visual` without a file open the **Changes** list at the top of the left panel. Each file shows its status (**A** added, **M** modified, **D** deleted, **R** renamed) and its summary.

The list appears immediately and the summaries fill in as they are computed (`checking N…`), so the viewer never blocks. Click a file to open its diff; the list stays in place. **↑** / **↓** move to the previous or next file (when the History panel is closed). **×** closes the list.

## Viewing a diff

Opening a diff shows the **Views** panel on the left with three views:

| View | Shows |
|---|---|
| **Diff** | The new version, with additions in green and removals in red |
| **Before** | The previous version (A) |
| **After** | The new version (B) |

Switching views keeps the zoom and position, so you can flip between versions of the same area.

The **Details** panel on the right has:

- **Summary**: facts about the open file or cell (for a layout: cell, PDK, polygons, labels, layers, size, transistors, nets).
- **Notices**: warnings from the format module, such as unresolved symbols.
- **Changes**: every functional change. A click frames the change on the canvas. In a layout diff, opens and shorts come first, in red.
- **Layers**: show or hide each layer (**Show all**, **Hide all**). Hidden layers stay hidden when you change cells.

The **Cells** panel (bottom of the left panel) lists a layout's cells, or a schematic's hierarchy (the schematic and the project sub-schematics it uses). It has a search box, **only top cells** (when there is more than one top cell) and **only with changes (N)** during a diff, which marks cells that changed, including changes inside them.

**Hierarchy navigation.** Double-click an instance of a sub-cell or sub-schematic to open it (the tooltip says `Double-click: open …`). **← Back** (next to the path above the canvas), **Backspace** or **Alt + ←** returns to the level above with the view it had, one level at a time.

What counts as a change in each format is described in [formats.md](formats.md).

## Controls

| Action | How |
|---|---|
| Pan | Drag. Releasing quickly keeps the view moving (momentum); a click stops it |
| Zoom | Mouse wheel (zooms at the cursor), pinch on a touchpad, Ctrl + wheel, or **+** / **−** |
| Touchpad | Two fingers sideways pan. Two fingers up/down zoom like the wheel, or pan with **Settings → Two fingers / wheel: Pan** (zoom then stays on pinch and Ctrl + wheel) |
| Fit the whole design | **Fit** or **F** |
| Show or hide texts (pins, names) | **Labels** or **L** |
| Which layer is each color | **Legend** or **G**: the layers of what is in view, bottom left |
| Highlight a layer | Hover its name in **Layers** or in the legend (the rest is dimmed); a click keeps it highlighted; click again or **Esc** to release |
| Polygon info | Hover a polygon: layer, size and area |
| Cursor position and scale | Status bar: `x`, `y` and the size of one pixel |
| Open a sub-cell or sub-schematic | Double-click its instance |
| Back to the level above | **← Back**, **Backspace** or **Alt + ←** |
| History panel | **History** or **H** |
| Next or previous file in a repository-wide diff | **↑** / **↓** (History panel closed) |
| Release highlights | **Esc** (layer, net, LVS selection) |

Keyboard shortcuts are ignored while a text field (search, filter, path) has the focus, so typing `f` in a search box does not fit the view. **Settings → Shortcuts** lists the main ones.

## Transistors and nets

For layouts from a supported PDK (SKY130, GF180MCU and IHP SG13G2, in GDS, OASIS and Magic), Riku recognizes transistors and nets with the PDK's device rules, without external tools.

- **Transistors layer.** Hidden when a file opens; tick it in **Layers** to draw each transistor gate in yellow with its model, W and L. **Summary** shows how many there are and how many are N and P.
- **Nets.** Hovering a polygon adds `net: NAME` to the tooltip (unlabeled nets get a generated name). Click a polygon to highlight its whole net in yellow and dim everything else; the status bar shows the net name and how many polygons it has. Click the same net again, click empty space, or press **Esc** to release it. **Summary** shows how many nets there are and how many have names.

> [!NOTE]
> In a cell with more than 2 million flattened polygons (a whole chip), transistors and nets are counted in **Summary** but not drawn, and net highlighting is not available: drawing them requires flattening the cell. Open a sub-cell to see them.

> [!NOTE]
> In version 0.2.0 the layout **Summary** labels and the name of the transistors layer (**Transistores**) are not translated yet and appear in Spanish.

See [formats.md](formats.md#transistors-and-nets) for how transistors and nets are extracted and compared.

## Waveforms

A `.raw` file (ngspice) opens in its own view with axes and units.

- **Diff**: B as a solid line over A dashed and, below it, the error B − A with a linked X axis. **Before** and **After** show one version each with the same zoom.
- **Details** lets you pick the **Analysis** and the signals to show (**Filter signals…**, **Only the ones that changed**, **Hide internal nodes**, **Show error (B − A)**), and shows a table with each signal's value in A and B and the difference.
- **Compare with** compares the open `.raw` against another `.raw` from the project directly, without going through Git.
- **Expressions** adds computed signals with ngspice syntax (`gain = v(out)/v(in)`, `db(v(out))`, `max(v(out))`). An expression that returns a curve is drawn like a signal; one that returns a number appears under **Measurements**. Expressions are remembered between sessions; the syntax is in [formats.md](formats.md).
- Zooming in redraws the curves with full detail for the visible span.

## History panel

**History** (or **H**) opens a panel at the bottom with the branch and merge graph (the same as `riku log --graph`), the refs and a semantic summary per commit, computed in the background.

- Click a commit to see its files; click a file to open its diff against the commit's first parent.
- **↑** / **↓** change the selected commit. **Enter** opens the commit's first file that can be opened.
- **Tab** moves the focus to the commit's files (marking the first one that can be opened); there **↑** / **↓** pick another file and **Enter** opens it. **Tab** or **Shift + Tab** again returns to the commits. With the History panel closed, Tab moves between buttons as usual.
- **Filter files: \*.gds** (a glob) or **Only this file** (the open file) simplify the graph; **Show all commits** clears the filter.
- The panel loads 200 commits at a time; **Load more** brings the next ones.

## Export and reload

- **Export → PNG / SVG** saves what you see as an image (the same renderer as `riku render`), at the size of the canvas and with the current theme. A message shows where it was saved, and the path is copied to the clipboard. Export is available for an open file or diff, not for a waveform comparison or the LVS view.
- **Reload** reads the open file, the project tree and the history again: use it after a commit or a change made outside the viewer.

## LVS view

With a schematic or a layout open, the **LVS** button in the top bar opens its pair side by side: the schematic on the left, the layout on the right, and the LVS panel on the right edge. The pair is chosen the same way as `riku lvs` (`[[lvs]]` in `.riku.toml`, or the schematic and layout with the same name); if the open file has no pair, a message says so. The LVS view always works on the files on disk, even if you opened it from a diff. **Close LVS** goes back to the schematic.

The panel has two tabs: **Links** (the default) and **Netgen**. What the checks mean is explained in [lvs.md](lvs.md).

### Links tab

This is the manual LVS: you say which schematic transistor is which layout transistor, and Riku checks everything that follows from those links. Every change is written to `lvs/<cell>.toml` immediately (the panel shows the file; `(new: it is written when you link)` if it does not exist yet).

Each transistor is outlined on both canvases:

| Color | Meaning |
|---|---|
| Green | Linked |
| Orange | Linked, with a different parameter or model |
| Grey | Not linked |
| Yellow | Selected |

**Selecting.**

- Click a transistor in the schematic to select it. If it is linked, its layout fingers are selected too. Click it again to deselect it.
- Click a finger in the layout. If it belongs to a link, the whole link and its schematic transistor are selected (cross-probing from the layout); if it is free, only that finger is selected.
- **Shift + click** adds a finger to the selection or removes it. You can pick fingers first and the schematic transistor afterwards: free fingers stay selected.
- Click a name in the lists below to select it and frame it on both sides, with enough context to see what it connects to.
- **Esc** clears the selection.

The selection box shows both sides with their W, for example `Schematic: M1 · W 4 µm` and `Layout: 2 fingers · W 2 µm`, so you can check the total width before linking.

**Actions.**

| Button | What it does |
|---|---|
| **Link** | Links the selected schematic transistor to the selected fingers. It replaces any earlier link of that transistor, and removes those fingers from other links |
| **Unlink** | Removes the link of the selected transistor |
| **Suggest** | Adds the links that follow without guessing (like `riku lvs --suggest`) |
| **Save positions** | After the layout moved, writes the current positions to the file (like `riku lvs --update`). Enabled only when Riku had to relocate links |

Below the buttons, the panel lists the progress (`Linked: 9 of 9 schematic transistors · 24 of 24 in the layout`), the verdict and every open item: **Shorts**, **Opens**, **Different parameters**, **Pins**, **Moved**, **Lost (not found in the layout)**, **Not linked (schematic)**, **Not linked in the layout: N fingers**, **Not checked** and **Linked**. Rows that name a schematic transistor are clickable.

### Netgen tab

Netgen is optional. The tab runs it only when you click **Compare with Netgen**, using the same comparison as `riku lvs --netgen` (Netgen and the PDK's setup must be installed; see [lvs.md](lvs.md#netgen-mode)).

The result shows the verdict, the device and net counts, the PDK and cell, any warnings, and the lists **Different parameters**, **Nets without a pair** and **Devices without a pair**. Click an item to find it on both sides: in the schematic, a net's wires and pins or a device's outline; in the layout, the net's polygons or the gates of the device (including every transistor Netgen merged in parallel with it). The rest is dimmed and the item is framed. Click it again or press **Esc** to release it. If the layout cannot place a name (a cell too large to compute its nets, or a layout without a known PDK), the panel says so and only the schematic side is highlighted.

In this tab, clicking a layout polygon highlights its net as in the normal view.

## Settings

The **Theme:** selector (**Light**, **Dark**, **System**) is in the top bar. The **Settings** menu contains:

| Setting | Effect |
|---|---|
| **Reduce motion** | No animations when fitting and no momentum after a drag |
| **Simplify when zoomed out** | In large layouts, whatever is smaller than a pixel is drawn as blocks in its layer color; zooming in shows everything. Turn it off to draw every polygon (slower). On by default |
| **Two fingers / wheel:** **Zoom** or **Pan** | What scrolling does. With **Pan**, zoom is on pinch or Ctrl + wheel |
| **Use the system window frame** | The desktop draws the title bar and buttons instead of Riku |
| **Language** | The interface language. New languages appear automatically when a translation is added |
| **Shortcuts** | The main keyboard and mouse shortcuts |

## Window and preferences

By default Riku draws its own window frame: the top bar is the title bar. Drag it to move the window, double-click it to maximize or restore, and use the minimize, maximize and close buttons on the right (close turns red under the pointer). Drag the window edges to resize. **Settings → Use the system window frame** switches to your desktop's frame.

The window opens maximized the first time; afterwards it restores the size and position you left it in.

Preferences are remembered between sessions: theme, language, labels, legend, **All files**, the settings above, the History panel height, recent files and folders, and waveform expressions. On Linux they are stored in `~/.local/share/riku-gui/app.ron`.

> [!TIP]
> If the window opens off-screen (for example after unplugging a monitor), close Riku and delete the `"window"` entry from `~/.local/share/riku-gui/app.ron`. Deleting the whole file resets every preference.
