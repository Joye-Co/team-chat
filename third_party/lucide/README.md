# Lucide icons

The desktop app draws its icons from [Lucide](https://lucide.dev) on systems without SF Symbols
(Windows; `docs/design/WINDOWS.md` §5, `apps/desktop/src/ui/sf_symbols/svgset.rs`).

- Version: 1.43.0, as bundled by the `gpui-kit-assets` crate (0.8.1, a dependency of `gpui-kit`);
  the SVG files are not copied into this repository.
- License: ISC, with the icons that come from Feather under MIT. The full text is in
  [LICENSE](LICENSE), copied from the crate's `LICENSE-LUCIDE`.
- `assets/icons/drawn/` holds the icons drawn for this app where Lucide has none. Four of them
  (`circle-alert-fill`, `circle-check-fill`, `circle-x-fill`, `triangle-alert-fill`) and
  `star-fill` reuse Lucide outlines, so the same license covers them; `circle-fill` and
  `desktop-mini` are this project's own (AGPL-3.0, like the rest of the repository).
