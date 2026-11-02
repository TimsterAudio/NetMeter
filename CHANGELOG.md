# Changelog

## Unreleased

- The dashboard: the new expand button (in the controls, or on the right-click menu) grows the meter into a window of tiles, and back. Each mode keeps its own size. Settings, Dashboard picks the tiles; a tile with nothing to show on this computer stays hidden.
- CPU: load over time, each core, the processor's name and temperature.
- Memory: in use over time, and swap on Linux.
- Disks: read and write rates over time, and how full each drive is.
- GPU: load over time, video memory, temperature and power for NVIDIA cards (Linux and Windows) and AMD cards on Linux; load alone for other cards on Windows.
- Temperatures and fans: every sensor Linux reports. On Windows, from LibreHardwareMonitor when it's running (Windows can't read them on its own), or the ACPI zones some computers report. °C or °F.
- USB: what's plugged in, with each link's speed on Linux, and a notice when a device is plugged in or out (can be turned off).

## 0.3.0 – 2026-11-01

- Settings open beside the meter, to its right, or to its left when it sits at the right edge of the screen, instead of wherever the system put them.
- A custom scale: choose Custom… under Scale and type the top of the graph in kb/s, Mb/s or Gb/s (or kB/s, MB/s, GB/s), say 300 Mb/s for your broadband's speed.
- Start when I log in, in Settings (Window): an autostart entry on Linux, a Run entry for your user on Windows; no administrator needed.

## 0.2.0 – 2026-11-01

- The Glass look, from the panel's review: frosted or smoked glass with the desktop showing through, no faceplate, Outfit type with light figures, and the controls (settings, stay on top, minimise, close) in a pill that appears only under the mouse.
- Download is green (Glass OK) and upload orange (Glass Pen 2) by default; both can be changed in Settings, from Glass's palette plus red, violet and ink.
- Settings open in a window of their own, with how see-through the glass is.
- Settings from 0.1.0 keep their choices; the hi-fi finish they name is no longer used.

## 0.1.0 – 2026-10-27

- First version: the TAW Net Meter (NM-001), a small desktop window of scrolling download and upload graphs, in the family's hi-fi look (Spool or Porcelain, light or dark).
- Internet and LAN apart (Linux), or all traffic; Internet over LAN, or either on its own.
- Overlaid or mirrored graphs; 1, 2, 5 or 10 minutes of history; a scale that follows the traffic or a fixed one, linear or logarithmic.
- Speeds in bits or bytes, with the unit chosen for you or fixed (kb/s, Mb/s, Gb/s; kB/s, MB/s, GB/s; or KiB/s, MiB/s, GiB/s); updated every ½, 1 or 2 seconds.
- All real adaptors, or one; stays on top; H or a double-click hides the title strip for a bare display you drag to move; right-click for quick choices; hover for any moment's rates.
