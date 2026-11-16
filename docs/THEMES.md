# Themes

Choose **Settings → General → Theme**. Luna remains the default.
Each theme is standalone: `luna/`, `daylight/` and `classic/` each contain a
complete palette, colors, fonts and all four stylesheets.
None inherits from another theme or depends on shared style files. They begin
with the same rounded layout, but editing one package affects only that theme.
Controls uses a flat background with the selected theme’s colors.
Gameplay and letterbox borders stay black in every theme.
Choose **System** in the same compact dropdown to follow desktop colors and
selection accents. Named themes use their own colors; there is no separate
system-colors checkbox.

## Make your own

1. Choose a starting theme, then click **Create theme…** and enter a name.
2. Edit `theme.json` in the folder that opens. Change palette colors, rating
   colors, or font weight. For the archived Qt frontend, edit `library.qss`, `settings.qss`, `gameplay.qss`
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

The archived Qt frontend supports the optional `controls-background` asset.
The built-in themes set it to `null` for a flat background.

Theme files contain JSON, QSS and images. They cannot run Python code. Invalid
packages are reported on reload and the app falls back to Luna without
changing the broken files. Native KDE decoration colors are generated from the
same palette in `<library>/cache/themes/`.

Classic uses charcoal surfaces with red selection accents. Luna and Daylight
use yellow ratings.

The setup assistant uses the selected theme’s `onboarding.qss` for its night-sky
presentation. `base/` files remain only as fallback styles for legacy packages.
