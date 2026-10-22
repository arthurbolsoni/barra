# Product

<!-- impeccable:product-schema 1 -->

## Platform

Windows 11 desktop (native Win32, Rust + Direct2D). Not web.

## Users

One user, the author (Arthur): developer on a single 2560x1080 ultrawide, keeping roughly 20 terminals plus browser and chat windows open at once. Keyboard is Alice layout: no two-modifier shortcuts.

## Product Purpose

Replace the Windows taskbar with a bar that organizes windows into groups instead of virtual desktops. Only the active group's windows sit on screen; the others are parked off-monitor (not minimized) so their DWM thumbnails stay live. Success: he never loses a window and can see every group at once.

## Positioning

Groups whose parked windows keep rendering, plus an overview (Alt+Tab / Win+Tab) that shows every group's live thumbnails in place. Windows virtual desktops cannot show all desktops at once.

## Operating Context

- Runs alongside snap-zones (his own zone-snapping tool for the same monitor); the bar is an AppBar, so snap zones and maximized windows stop at its edge.
- Hides the Windows taskbar and takes over the tray icons of other programs (Shell_TrayWnd trick from ManagedShell/RetroBar).
- Fullscreen games/video and elevated windows keep native Windows behavior.

## Capabilities and Constraints

- Bar: groups, the active group's windows, tray icons, network/volume, clock.
- Overview: cards per group with live thumbnails, filter by typing, drag a window between groups, rename by double click.
- Progress and badge from ITaskbarList3 on window buttons.
- Jump lists and thumbnail toolbar buttons are out of scope.

## Brand Commitments

- Visual reference pinned by the user: Omarchy Linux (Hyprland + Waybar by DHH). Bar at the top, thin, CaskaydiaMono Nerd Font, Omarchy theme colors.
- Every Omarchy theme must be selectable; the user picks.
- Window buttons: icons only, no titles.
- No Windows logo / Start button on the bar.

## Product Principles

- Never lose a window: anything parked comes back on exit, crash, shutdown or restart.
- Keyboard first, single-modifier: Alt+Tab and Win+Tab, no chords.
- Out of the way: the bar is thin and quiet; the overview is where the information lives.
