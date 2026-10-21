---
version: 1
slug: "src-desenho-rs"
primary_target: "src/desenho.rs"
related_targets: ["src/main.rs"]
---

Scope: the whole bar UI (top bar, tooltip, overview) in src/desenho.rs. Mode: Operate.

Audience/job: one developer on a 2560x1080 ultrawide switching among ~20 terminals grouped by project; reads the active group, the windows in it, tray, clock; opens the overview to find any window.

Constraints: native Win32 + Direct2D; native Win32 menus stay; JetBrainsMono Nerd Font installed; all Omarchy themes selectable from the bar menu.

## Direction contract

THESIS: the bar is Omarchy's Waybar transplanted to Windows: a 26 px strip at the top, information as text and glyphs, no buttons-as-pills. Refuses the Windows taskbar arrangement (bottom, rounded tiles, titles, Start).

OWN-WORLD: Omarchy theme roles (background, foreground, dark_foreground, muted, accent, selection, red/yellow/green) from the user-selected theme, default tokyo-night (#1a1b26 / #a9b1d6 / #7aa2f7). JetBrainsMono Nerd Font 12 px everywhere, Nerd Font glyphs for system icons. Zero corner rounding, 2 px borders, Hyprland gaps (10 px outer, 5 px inner) in the overview.

STORY: glance up, read which group is active and which window has focus, read the time; Alt+Tab shows every group as Hyprland-style tiled frames with live thumbnails.

FIRST VIEWPORT: top strip 26 px. Left: group names (numbers by default) spaced like workspaces, active in accent, empty ones muted; a plus glyph for a new group; then the active group's window icons at 16 px, focused one with a 2 px accent underline. Center: weekday and time. Right: tray icons 16 px, network and volume glyphs.

FORM: pinned by the user (Omarchy Linux); no concept-seed roll, brief-pinned direction. Signature move: the overview renders groups as Hyprland frames, 2 px accent border on the focused frame, sharp corners, gaps.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
