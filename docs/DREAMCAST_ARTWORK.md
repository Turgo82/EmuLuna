# Dreamcast artwork

These two new assets were created for EmuLuna with the built-in `image_gen`
tool. They are separate from the OpenEmu console icons and the imported
Pinapple_Graphics controller illustrations. The controller has no branding;
the console icon uses an orange spiral to make it recognizable at sidebar size.
Transparency is preserved in the exported PNGs.

- Console icon: `emuluna/data/icons/dreamcast.png`, 32 × 32 PNG, trimmed and
  reduced with area averaging from the generated pixel-art image. Transparent
  padding is preserved. Enlarged previews use nearest-neighbor scaling.
- Controller: `emuluna/data/controllers/dreamcast.png`, 1100 × 1005 PNG,
  proportionally scaled from the generated controller image. Interactive
  regions are registered in `emuluna/data/controllers/layouts.json`.

## Original console generation prompt (superseded)

Use case: stylized-concept. Asset type: a single Sega Dreamcast console sidebar icon for EmuLuna, matching a set of small shaded pixel-art console icons. Draw only the off-white Dreamcast console, centered, on a genuinely transparent background. Straight-on shallow elevated front view: compact nearly square white/light-gray case with the large circular lid visible on top, four small controller sockets along the front, small dark ventilation details, subtle gray pixel shading and a clear dark-gray outline. Classic late-1990s desktop pixel icon style, like a hand-crafted 32x32 or 48x48 sprite enlarged with nearest-neighbor pixels: consistent square pixel blocks, crisp staircase edges, limited palette, no antialiasing within the artwork. Correct Dreamcast silhouette. No branding, no logo, no spiral, no words or letters, no controller, no cable, no extra props, no cast shadow outside the console. Frame the console tightly with equal transparent margins. This is a new asset, not a replacement for any existing console icon.

## Revised console prompt — 2026-10-03

Built-in image tool, edit mode. The previous Dreamcast PNG was the edit target;
the existing PlayStation and Saturn PNGs were style references.

Redesign the Dreamcast console icon (image 1, edit target) so it is clearly recognizable at tiny 32x32 sidebar size. Images 2 and 3 are style references only: match their classic shaded desktop pixel-art console icons. Output ONE isolated Dreamcast console icon on actual transparent background, tightly centered with a small even margin, no other images or objects. Use a slightly elevated near-top-down view with the front edge visible, similar to the PlayStation reference. A distinctive light ivory/off-white nearly square Dreamcast shell with curved front corners; a very clear LARGE CIRCULAR DISC LID centered on top, strong medium-gray circular seam and a small orange spiral mark on the lid; round power/open buttons at top-front corners; four evenly spaced dark rectangular controller sockets along the visible front edge; subtle orange power indicator. Give the silhouette and ports a crisp dark gray outline, warm white highlights and medium gray side shading so it remains readable on a dark sidebar. Build the artwork as a coherent hand-drawn 32x32 or 48x48 pixel sprite enlarged with nearest-neighbor square pixels, with restrained shaded clusters like the reference icons. Prioritize a bold identifiable lid and four ports, NOT speckled fine details or noisy texture. No photorealism, no soft edges, no blur, no gradients, no faint white-on-white disc seam, no cable, no controller, no drop shadow outside the console, no letters, no product name, no watermark. Replace the weak anonymous box in image 1, retaining its single-console transparent icon purpose.

## Controller generation prompt

Use case: product-mockup. Asset type: a brandless Dreamcast-style controller illustration for EmuLuna's controller-binding settings. Create ONE complete controller, centered and tightly framed with even small transparent margins, on a truly transparent background. Face-on orthographic top view, exact bilateral controller body with realistic clean illustration shading, light gray/off-white plastic, large broad upper body, two downward tapered hand grips. The characteristic Dreamcast design: exactly one recessed light-gray analog stick at the upper left, exactly one dark gray cross D-pad below it at the lower left, a rectangular recessed VMU screen/module window at upper center with a plain blank blue-gray LCD screen, a small triangular dark gray Start button below the VMU in the center, four colored round buttons at right in a diamond, labeled correctly: green Y at top, blue X at left, red B at right, yellow A at bottom. Show the tips of two gray rear shoulder triggers at the upper left and upper right, unobstructed enough for settings highlights. Slightly sculpted plastic, crisp edges, subtle bevels, readable button letters, no artistic perspective or tilt. Match clean controller reference illustrations used in emulator settings. IMPORTANT: no Sega name, no Dreamcast name, no brand logos, no spiral symbol, no watermark, no product name, no decorative badge; the only visible text may be A B X Y on their own buttons. No hands, no props, no cable, no background, no cast shadow. Do not add a right analog stick.
