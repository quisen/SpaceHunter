# SpaceMonger 1.4 – reverse-engineering notes

Source: `SpaceMonger.exe` (217 088 bytes, PE32 i386, linked 2000-10-16, MSVC 6.0 + statically linked MFC 4.2, no symbols).
Author: Sean Werkema / "65 Systems" (© 1997-2000), freeware. Version resource: 1.4.0.1. Registry key `Software\65\SpaceMonger\1.3`.
Method: PE header/imports/resource dump (`tools/extract_resources.py`), string analysis, and annotated disassembly (`objdump -d -M intel`).
Tags: **[certain]** read from the code, **[inferred]** interpretation.

## 1. Program shape
- Single-threaded MFC dialog-less SDI app: `CMainFrame`, `CFolderView` (the map), `CMainToolBar`, `CSettingsDialog`, `CFolderDialog`, `CTipWnd` (custom tooltips), `CFreeView` (free-space/summary).
- Imports only Win32 basics: `FindFirstFileA/FindNextFileA`, `GetDiskFreeSpaceExA` (dynamic, fallback `GetDiskFreeSpaceA`), `DeviceIoControl` (reparse points), `SHFileOperationA` (delete to recycle bin), `ShellExecuteExA`, `SHGetFileInfoA`, `GDI32` rectangles/text (no bitmaps for the map – everything is `FillRect/FrameRect/TextOut`), `SetTimer`/`Sleep` (animation).
- Resources: 6 bitmaps (toolbar strip with labels Open, Reload, Zoom Full, Zoom In, Zoom Out, Free Space, Run or Open, Delete, Setup, About – `assets/toolbar.png`; four 128×48 "scanning" animation frames – `assets/scan-animation.png`), 3 icons, 4 dialogs (About, drive list `SysListView32`, scan progress `msctls_progress32`, Settings with `msctls_trackbar32`), a hand-built menu (no MENU resource), English + French string tables (`Rouvrir`, `Vrombir`…).
- Settings (registry): `density`, `bias`, `file_color`, `folder_color`, `animated_zoom`, `show_rollover_box`, `show_name_tips`, `show_info_tips`, `infotip_flags` (Attributes, File Size, Date/Time, Icon, Small Icon, Filename, Full Path), `infotip_delay`, `nametip_delay`, `save_pos`, `disable_delete`, `auto_rescan`, window rect, language.

## 2. Scanner (0x402470) [certain]
- Recursive `FindFirstFile("<dir>\*.*")` / `FindNextFile`; skips `.` and `..`.
- **Reparse points** (attr 0x400): opens the directory, `DeviceIoControl(FSCTL_GET_REPARSE_POINT = 0x900A8)` and skips junction/mount-point tags (`0x80000000`, `0xA0000003`) – prevents loops.
- **Size = size on disk**: file size (64-bit from high/low) rounded up to the cluster size (`(size + mask) & ~mask`).
- Per-folder node (0x38 bytes) with parallel arrays: `names*`, `child*`, `size64*`, attr, date; count and capacity (grows ×2); folder total at +0x20.
- Names written entirely in upper case are lower-cased after the first letter.
- UI is kept alive by pumping messages every 200 ms (`GetTickCount`) and updating a progress bar – there is no thread.
- After each folder: children **sorted by size descending** with an LSD radix sort (8 passes × 256 buckets, 0x401fc0).
- Free space is a synthetic root child named `<<<<<<<<<<<<<<<<<<<<` whose size is the free bytes of the drive; it sorts with the others and is hidden unless "Show Free Space" is on.

## 3. Layout algorithm (0x4059b0 / 0x405a40) [certain]
**Not** squarified and not slice-and-dice: a recursive **balanced binary partition**.
```
Split(rect, items sorted by size desc):
  A, B = [], []                         # greedy LPT bisection
  for item in items: (A if sumA <= sumB else B).add(item)   # zero-size items skipped
  bias in -20..20, fw = 8 + max(bias,0), fh = 8 + max(-bias,0)
  if w*fw/8 > h*fh/8: cut the width  : wA = w*sumA/(sumA+sumB)   (A left,  B right)
  else:               cut the height : hA = h*sumA/(sumA+sumB)   (A top,   B bottom)
  for each half: if |group|>1 and half.w>minW and half.h>minH: recurse
                 elif |group|==1 and half passes minW/minH: draw the item (folders recurse into rect inset 3,12,-6,-15)
                 else: one flat grey filler block for the whole group
```
- Density ("Too Many … Too Few Files") is just the `minW × minH` table: 96×64, 64×48, 48×32, **32×24 (Normal)**, 24×16, 16×12 – a block is emitted only if `w > minW && h > minH`.
- Folders: 12 px title strip, 3 px side/bottom border. Root fills the client area (`w-1, h-1`). Direction is purely aspect based (no alternation by depth).
- Cells are an array in pre-order; hit-testing walks it and the deepest match wins; clicking a folder zooms into it.

## 4. Rendering [certain]
- Bevel: 1 px light top/left, 1 px dark bottom/right, 1 px black outline, base colour fill. No gradients/cushions. Folder = base-coloured header + 1 px frame; children drawn over a window-background interior.
- Colour schemes (`file_color`, `folder_color`): **Rainbow** (by absolute depth & 7), **Windows Colors** (BTNFACE/BTNHIGHLIGHT/BTNSHADOW), and ten constants (White, Light/Dark Gray, Red, Orange, Yellow, Green, Aqua, Blue, Violet). Rainbow table (base/light/dark): `255,127,127 / 255,191,191 / 191,127,127`, `255,191,127`, `255,255,0`, `127,255,127`, `127,255,255`, `191,191,255`, `191,191,191`, `255,127,255` (full table in `crates/app/src/colors.rs`).
- Labels via `TextOut`: folder names left-aligned in the title strip; file names centred if they fit; blocks ≥ 48×36 also show the formatted size on a second line. Free space is drawn as `<Free Space: 12.3%>`.
- Roll-over box highlights the hovered block (base→light, light→white).

## 5. Interaction
- Left click on a folder → **Zoom In**; toolbar/menu: Zoom Out (parent), Zoom Full (root), Reload, Open Drive, Show Free Space, Run/Open (`ShellExecute`), Delete (`SHFileOperation`, optional "Auto rescan on delete" and "Disable Delete command").
- **Animated zoom** = an XOR wire-frame rectangle morphing from the clicked cell to the client rect in 8 steps (25 ms each), drawn then erased – no intermediate re-render.
- Tooltips: file-info tips (attributes, size, date, icon, name, full path) and name tips with configurable delay.

## 6. What SpaceHunter keeps / changes
| SpaceMonger 1.4 | SpaceHunter |
|---|---|
| Single-threaded `FindFirstFile` | Parallel work-stealing scanner (rayon), `read_dir` + metadata, hard-link de-dup (Unix), reparse/symlink skipping |
| Size on disk via cluster rounding | Same (`GetDiskFreeSpaceW` on Windows, `st_blocks` on Unix); toggle for logical size |
| Binary partition layout | Kept as **Classic** (exact algorithm, density table, bias −20…20); added **Squarified** |
| GDI rectangles, instant redraw | Cached gouraud mesh (egui/OpenGL), ~0.3 ms layouts; 3D instanced renderer with depth buffer |
| XOR wire-frame zoom | Interpolated re-layout zoom (0.28 s, ease-out) |
| Windows only | Windows `.exe`, Linux/macOS build, and an installable PWA (WASM) |
| — | 2D ⇄ 3D view, size heat-map, file-type colours, largest-files panel, breadcrumb, light/dark |

## 7. Uncertain
Which end of the Bias slider is labelled "Vert"/"Horz"; exact combo ordering of schemes (inferred from tables); a few edge cases of the click handlers and context menu.
