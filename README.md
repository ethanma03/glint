# Glint

A fast, minimal image viewer for Windows, made for game sprites and screenshots.

- **Crisp pixels**: zoom is measured in real screen pixels and snaps to clean steps (100%, 200%, 300%...). Small sprites open at the largest integer scale that fits. A pixel grid appears from 600%.
- **Opens almost anything**: PNG/APNG, JPEG, GIF, WebP, BMP, ICO, TGA, DDS, QOI, TIFF, EXR/HDR, PNM, SVG and Aseprite are built in. AVIF, HEIC, JPEG XL, PSD, XCF and camera RAW go through ImageMagick. Formats are detected by content, so a JPEG named `.png` still opens.
- **Sprite tools**:
  - animation playback with a frame scrubber and Aseprite tags;
  - a sprite-sheet grid overlay with cell indices and a live cell-animation preview;
  - a pixel inspector (coords + hex);
  - backgrounds: checker, black, white or magenta.
- **Compare**: diff, side-by-side, onion skin and swipe modes, with AE, RMSE and PSNR readouts. In Explorer, select two images and choose "Compare with Glint".
- **Scriptable**: `glint ctl …` controls the open window, `glint snap` captures it, and `glint edit` / `glint magick` run ImageMagick with preview and undo.

## Install
```powershell
pwsh scripts\install.ps1     # build, install to %LOCALAPPDATA%\Programs\Glint, register file types
pwsh scripts\update.ps1      # later: pull, rebuild, reinstall
```
Then in the Settings page that opens, click **Set default** next to Glint.

## Keys
Press **F1** in the viewer for the full list. The basics:
- `←/→` browse the folder
- wheel: zoom; `0` fit; `1`–`9` sets 100–900%
- `G` grid, `B` background, `Space` play/pause, `,`/`.` step frames
- `I` details, `C` copy image, `Ctrl+P` command palette
