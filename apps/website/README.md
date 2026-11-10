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
| `src/main.js` | Scroll reveals, the card gallery, and the clickable app demos |
| `src/img/` | WebP screenshots in light and dark pairs (`*-light.webp`, `*-dark.webp`) and the app icon |
| `src/img/ui/` | The screens of the clickable demos |

## Screenshots

Every screenshot comes from the app's own screenshot self-test, with the sample team of
`apps/desktop/src/fake/` (no real data):

```bash
desktop --screenshot <dir> --theme both --size 1440x900 --scenes desk,topic-v2,…
```

Then convert with `cwebp -q 88 -m 6`. Card images (`img/card-*`) are crops of the card gallery
scenes (`compose-*`, `cards-v2-*`) taken at `--size 1440x1700`.

## Clickable demos

A `<div class="shot demo" data-start="<screen>">` becomes a clickable copy of the app. The screens and
their click areas live in `G` in `src/main.js`: each area is `[left, top, width, height]` in percent of
the window, the screen it opens, and its tooltip. `SB` holds the sidebar areas shared by every main
window screen. When you add a screen, capture it at 1440x900 so the percentages line up.
