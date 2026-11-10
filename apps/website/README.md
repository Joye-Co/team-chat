# Website

The product homepage: one static page in plain HTML, CSS and JS, no framework and no runtime
dependencies.

| Command | What it does |
| --- | --- |
| `bun run dev:website` | Builds into `dist/`, rebuilds on every save under `src/`, serves `http://127.0.0.1:4321/` |
| `bun run build:website` | Builds into `apps/website/dist/`, ready for any static host |

## Layout

| Path | What it holds |
| --- | --- |
| `src/index.html` | The page. Write `{{PRODUCT_NAME}}` wherever the product name appears; the build fills it from `product.json`. |
| `src/styles.css` | Tokens for light and dark mode, layout, and the scroll motion (only under `.motion`, which `main.js` adds unless the viewer asked for reduced motion) |
| `src/main.js` | Scroll reveals, the card gallery, the comparison (one panel per alternative, each with its own rows; the tabs pick the panel), the sliding pill of the tab rows, the nav link of the section in view, the member builder (chips drive the member card) and the waitlist form and the clickable app demos |
| `src/img/` | WebP screenshots in light and dark pairs (`*-light.webp`, `*-dark.webp`) and the app icon |
| `src/img/ui/` | The screens of the clickable demos |

## Screenshots

Every screenshot comes from the app's own screenshot self-test, with the sample team of
`apps/desktop/src/fake/` (no real data):

```bash
desktop --screenshot <dir> --theme both --size 1440x900 --scenes desk,topic-v2,…
```

Then convert with `cwebp -q 88 -m 6`. `settings-machines` is the settings window without its
toolbar (`-crop 0 190 1280 900`). Card images (`img/card-*`) are crops of the card gallery
scenes (`compose-*`, `cards-v2-*`) taken at `--size 1440x1700`.

## Phones

Up to 640 px a `.shot` with the class `pan` keeps a readable width and scrolls sideways instead of
shrinking; `data-pan` (0 to 1) says where it starts, so the subject is in view. Tab rows scroll in one
line, and the comparison stacks each row (theirs, then ours).

## Waitlist

The page has no download link yet. The buttons lead to the form at the end (`#waitlist`), which talks
to `/api/waitlist` on the same host: `GET` answers `{ "count": n }`, `POST { "email": "…" }` books
the address and answers the new count (a repeated address is not counted twice). The static build
does not include that endpoint: the host that serves the page provides it. Without it (also in
`bun run dev:website`) the form says the booking did not go through and the count stays hidden. The
count shows from 10 bookings on.

## Pins and legends

A screenshot can carry numbered pins and a legend under it: `<div class="pins">` inside the `.shot`
holds one `<i class="pin" data-n="1" style="left:..%;top:..%">` per point (percent of the image), and
the `<ol class="legend">` after the shot explains each number. Pointing at a legend item lights its
pin. On a clickable demo the pins show only on the start screen.

## Clickable demos

A `<div class="shot demo" data-start="<screen>">` becomes a clickable copy of the app. The screens and
their click areas live in `G` in `src/main.js`: each area is `[left, top, width, height]` in percent of
the window, the screen it opens, and its tooltip. `SB` holds the sidebar areas shared by every main
window screen. When you add a screen, capture it at 1440x900 so the percentages line up.
