# Glint

Personal image viewer for Windows (Rust, eframe/egui on wgpu). Tuned for game sprites and screenshots, and built to be driven from the command line, especially by Claude Code.

## Layout
- `src/main.rs`: `glint.exe`, a console CLI. It runs headless ops and remote-controls the viewer over a named pipe.
- `src/gui_main.rs`: `glintw.exe`, the windowed viewer (no console). File associations point here. It is single-instance: a second launch forwards its files to the first window and exits.
- `src/ipc.rs`: the named pipe `\\.\pipe\glint-<user>`. The protocol is one JSON line per request and one per reply.
- `src/app/`: the viewer.
  - `commands.rs`: the command language shared by `glint ctl`, the Ctrl+P palette and keybindings. Add new commands here and to `COMMANDS`.
  - `view.rs`: zoom/pan. Zoom is in *physical* px per image px, and the origin is snapped to the pixel grid.
  - `draw.rs`: the canvas, checkerboard, grid, sheet overlay and compare modes.
  - `hud.rs`: overlays, palette, help and modals.
  - `layer.rs`: GPU textures. Images over the max texture size are split into tiles.
- `src/decode/`: content sniffing → `image` crate / resvg / asefile / ImageMagick fallback (via stdin + PAM). Every decoder runs behind `catch_unwind`.
- `src/ops.rs`: headless info, compare, edits through magick (with an undo journal) and sheet slicing.
- `src/analysis.rs`: stats, diff metrics, the diff image and sprite-sheet grid math.

## Build / test / install
- `cargo build` (debug; deps are optimized via profile override). `cargo test` runs the decoder tests against `tests/fixtures`.
- Regenerate the fixtures with `python scripts/make_fixtures.py` (needs `magick`).
- Install: `pwsh scripts/install.ps1`. This builds release, copies to `%LOCALAPPDATA%\Programs\Glint`, adds it to PATH and registers per-user file associations.
- Update after code changes: `pwsh scripts/update.ps1`. It rebuilds, reinstalls and reopens the last image.
- Windows protects the default-app choice. Setting the default is a one-time click in Settings → Default apps → Glint → "Set default".

## Driving the viewer (for Claude)
All commands print JSON. Exit code 2 means no viewer is running.
```
glint <file>                          # open (starts the viewer if needed; waits until decoded)
glint state                           # file, zoom_percent, visible_rect, frame, cursor_px/rgba, compare metrics...
glint snap -o out.png [--ui]          # PNG of exactly what the window shows -> Read it to "see" the viewer
glint ctl zoom 800 | zoom fit | center 16 16 | grid on | bg magenta | smooth off
glint ctl frame 3 | play off | tag walk | speed 0.5
glint ctl sheet 32x32 offset 0,0 gap 0,0 play 0-7 fps 12 | sheet off
glint ctl compare a.png b.png diff|side|onion|swipe | mode onion | split 0.3 | compare off
glint ctl help                        # every viewer command
glint info sprite.png --json          # dims, frames/delays, Aseprite tags, unique colors, alpha bbox
glint compare a.png b.png --json --no-show [--diff-out d.png]   # AE / RMSE / PSNR / diff bbox
glint sheet atlas.png --grid 16x16 --json [--export outdir]     # per-cell rects, empty cells, bboxes
glint edit <op> [args] [-i file] [-o out | --in-place]          # crop x,y,w,h | rotate 90 | flip h | scale 4x | trim | pad WxH | key #ff00ff 5% | flatten | colors 16 | grayscale | strip | convert webp
glint magick [-i file] [-o out | --in-place] -- <magick args>   # arbitrary magick pipeline
glint undo                            # restore the file before the last --in-place edit
```
- Without `-o`/`--in-place`, edits go to a temp preview that opens in the viewer. The original is untouched.
- `--in-place` backs up to `%LOCALAPPDATA%\Glint\undo`, and the viewer reloads automatically.
- Without `-i`, `edit`/`magick` operate on the image currently open in the viewer.
- The viewer also live-reloads when a file it shows changes on disk, so a plain `magick in.png ... in.png` shows up on its own.
