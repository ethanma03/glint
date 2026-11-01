Glint
=====

Glint is a fast image viewer for game sprites and screenshots for Windows.
It is part of openwith: https://openwith.dev

Installing
----------
1. Unzip this folder anywhere, for example to your Downloads folder.
2. Double-click Install.cmd.

The installer copies Glint to %LOCALAPPDATA%\Programs\Glint, adds the `glint`
command to your PATH and registers Glint for images. It installs for your
user account only and does not need administrator rights.

At the end, Windows shows one UAC prompt. If you approve it, Glint becomes the
default app for images the next time you sign in. If you decline it,
Glint is still installed and appears under "Open with".

Glint keeps running in the system tray after you close its window, so the next
file opens instantly. It also starts at sign-in. To turn this off, run the
installer from a terminal with -NoBackground:

    powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -NoBackground

Windows SmartScreen
-------------------
Glint is not code-signed yet, so Windows may say that it "protected your PC"
the first time you run something from this folder. Click "More info", then
"Run anyway". The source code is public at https://github.com/ethanma03/glint.

Uninstalling
------------
Double-click Uninstall.cmd in this folder, or run
%LOCALAPPDATA%\Programs\Glint\scripts\uninstall.ps1. Your settings stay in
%LOCALAPPDATA%\Glint in case you reinstall.

Press F1 inside Glint to see every keyboard shortcut.
