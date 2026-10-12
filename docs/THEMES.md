# Themes

Choose **Settings → General → Appearance**. Luna remains the default.
Each theme is standalone: `luna/`, `daylight/` and `classic/` each contain a
complete palette, colors, fonts, controller wood texture and all four stylesheets.
None inherits from another theme or depends on shared style files. They begin
with the same rounded layout, but editing one package affects only that theme.
All three use OpenEmu’s original golden wood in Controls with readable brown text.
Gameplay and letterbox borders stay black in every theme.
“Use system theme colors” optionally follows the desktop palette instead.

## Make your own

1. Choose a starting theme, then click **Create theme…** and enter a name.
2. Edit `theme.json` in the folder that opens. Change palette colors, rating
   colors, or the wood asset. Edit `library.qss`, `settings.qss`, `gameplay.qss`
   or `onboarding.qss` to change its styling.
3. Click **Reload themes**. The selected theme updates in open windows.

Custom themes live in `<library>/themes/<theme-id>/`. Built-in packages live
in `emuluna/data/themes/`. **Create theme…** copies the entire selected theme,
including its assets and styles, with no `extends` reference. The copy remains
independent even if the source changes later.

For example, create Ocean from Daylight, then edit its complete `theme.json`:
set `palette.highlight` and `palette.accent` to `#4ab5cf`, and set
`colors.rating-filled` to `#f2c94c`. Keep the other palette entries in the file.
Names are display labels; IDs must be unique lowercase letters, numbers,
underscores or hyphens. The format is currently `1`.

Saved old IDs migrate from `purple` to `luna`, `light` to `daylight`, and
`openemu` to `classic`. Old partial custom packages are still readable for
compatibility; creating a copy makes a standalone package.

Palette roles: `window`, `window-text`, `base`, `alternate-base`, `text`,
`button`, `button-text`, `highlight`, `accent`, `highlighted-text`,
`placeholder-text`, `tooltip-base`, `tooltip-text`, `mid`, `midlight`, `light`,
`dark`, `link`, `link-visited`, `disabled-text`.

Extra colors include `rating-filled`, `rating-empty`, `controls-surface`,
`controls-text`, `controls-subtle`, `controls-accent`, `controls-popup`,
`controls-border`, and `controls-scrollbar`. Styles can use `palette(highlight)`
or `theme(controls-text)`; the app resolves them when applying the theme.

`fonts` can override `game-title-weight` with a value from 100 to 900 (default
700). Other widget font styling can be changed in QSS.

The only asset slot is `controls-background`; set it to `null` for a flat
surface, or to an image inside your theme folder. Images referenced by
`url(...)` in QSS also stay inside that folder. A `gameplay` stylesheet override
is supported alongside `library` and `settings`.

Theme files contain JSON, QSS and images. They cannot run Python code. Invalid
packages are reported on reload and the app falls back to Luna without
changing the broken files. Native KDE decoration colors are generated from the
same palette in `<library>/cache/themes/`.

## Controls wood asset

All three built-in themes use the original `controls_background@2x.jpg` from the
OpenEmu source archive, copied unchanged into each theme as
`controls-wood.jpg`. `openemu-wood.json` records its upstream location,
SHA-256 and attribution. Every theme contains its own copy.
The Controls palette applies to all labels, custom-painted buttons, inputs and
popups, including widgets recreated when the console or input device changes.

## OpenEmu reference

The Classic package follows the source's `accent.colorset` (`#D12733`),
`CollectionViewItem.swift` selection borders and the native toolbar/sidebar
controls. The original relies on macOS dynamic colors and materials, so the
Linux package provides fixed charcoal equivalents rather than claiming to
reproduce macOS rendering pixel for pixel. Controls uses the upstream golden wood texture. Luna and Daylight retain yellow ratings.

References: [accent asset](https://github.com/OpenEmu/OpenEmu/blob/master/OpenEmu/Graphics.xcassets/accent.colorset/Contents.json),
[collection selection](https://github.com/OpenEmu/OpenEmu/blob/master/OpenEmu/CollectionViewItem.swift),
[toolbar](https://github.com/OpenEmu/OpenEmu/blob/master/OpenEmu/LibraryToolbar.swift).

The setup assistant uses the selected theme’s `onboarding.qss` for its night-sky
presentation. `base/` files remain only as fallback styles for legacy packages.
