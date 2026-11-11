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
| `src/en.html` | The English page, served at `/en`. Same sections as `index.html`: change both together. `main.js` picks its strings from `<html lang>`. |
| `src/styles.css` | Tokens for light and dark mode, layout, and the scroll motion (only under `.motion`, which `main.js` adds unless the viewer asked for reduced motion) |
| `src/main.js` | Scroll reveals, the screenshot tours, the comparison (one panel per alternative, each with its own rows; the tabs pick the panel), the sliding pill of the tab rows, the nav link of the section in view, the member builder (chips drive the member card) and the waitlist form |
| `src/img/` | WebP screenshots in light and dark pairs (`*-light.webp`, `*-dark.webp`) and the app icon |
| `src/img/ui/` | The screens of the screenshot tours |

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

Up to 760 px a tour puts the frame above its steps and zooms in further, so the ringed part stays
readable. Up to 640 px the machines screenshot keeps a readable width and scrolls sideways, tab rows
scroll in one line, and the comparison stacks each row (theirs, then ours).

## Waitlist

The page has no download link yet. The buttons lead to the form at the end (`#waitlist`), which talks
to `/api/waitlist` on the same host: `GET` answers `{ "count": n }`, `POST { "email": "…" }` books
the address and answers the new count (a repeated address is not counted twice). The static build
does not include that endpoint: the host that serves the page provides it. Without it (also in
`bun run dev:website`) the form says the booking did not go through and the count stays hidden. The
count shows from 10 bookings on.

## Screenshot tours

A `<div class="player">` is a tour of the app: an `<ol class="pl">` of steps and a `.pf` frame. Each
step `<li class="ps">` names its screen in `data-img` (the path without `-light.webp`/`-dark.webp`)
and the part to point at in `data-f` (left, top, width, height in percent of the screenshot). The
frame zooms toward that part and rings it; when the step's bar fills, the next step starts (3 s a step). On
a wide window a tour sticks while you scroll through it (a tour section sticks whole below the nav;
the hero's tour, in `.hero-tour`, sticks alone in the middle of the window), and each stretch of scroll
moves one step (only when the section fits the window). Pointing at the tour, or scrolling it out
of view, holds it; with reduced motion nothing plays on its own. A
tour with the class `fit` shows whole images (the cards) without zoom. Screens come from the app at
1440x900, so the percentages line up.
