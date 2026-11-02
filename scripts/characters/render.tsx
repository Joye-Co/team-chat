import { mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { renderSvg, CHARACTER_IDS, STATUS_IDS, isEmotionId } from './library'

export const PAIR: Record<string, string> = {
  idle: 'calm', working: 'focus', 'needs-input': 'curious', success: 'happy', error: 'puzzled', offline: 'sleepy',
}
export const SIZES = [16, 20, 24, 32, 48, 64, 128]

for (const e of Object.values(PAIR)) if (!isEmotionId(e)) throw new Error(`unknown emotion ${e}`)

/** Smaller files, same picture: drop data-/aria/style attributes and round coordinates to 0.1 of an 800-unit box. */
export function minify(svg: string) {
  // The root style carries the offline dimming as CSS; resvg (GPUI) reads it more reliably as a group.
  const root = /^<svg([^>]*?) style="([^"]*)"([^>]*)>/.exec(svg)
  if (root) {
    const style = root[2]
    const filter = /filter:([^;]+)/.exec(style)?.[1]
    const opacity = /opacity:([^;]+)/.exec(style)?.[1]
    const attrs = (filter ? ` filter="${filter}"` : '') + (opacity ? ` opacity="${opacity}"` : '')
    svg = `<svg${root[1]}${root[3]}>` + (attrs ? `<g${attrs}>` : '') + svg.slice(root[0].length, -'</svg>'.length) + (attrs ? '</g>' : '') + '</svg>'
  }
  return svg
    .replace(/ (data-[a-z-]+|aria-hidden|focusable|style|overflow)="[^"]*"/g, '')
    // only coordinates (|n| >= 10) are rounded; opacities, offsets and scales keep their precision
    .replace(/-?\d{2,}\.\d{2,}/g, (n) => {
      const r = Math.round(Number(n) * 10) / 10
      return Object.is(r, -0) ? '0' : String(r)
    })
}

export function render(character: string, status: string, size: number) {
  return minify(renderSvg({ character, emotion: PAIR[status] as any, status: status as any, size, decorative: true, xmlDeclaration: false }))
}

if (import.meta.main) {
  const out = process.argv[2]
  if (!out) throw new Error('usage: render.tsx <out dir>')
  rmSync(out, { recursive: true, force: true })
  let total = 0, n = 0
  for (const c of CHARACTER_IDS) {
    mkdirSync(join(out, c), { recursive: true })
    for (const s of STATUS_IDS) for (const px of SIZES) {
      const svg = render(c, s, px)
      writeFileSync(join(out, c, `${s}-${px}.svg`), svg + '\n')
      total += svg.length + 1; n++
    }
  }
  console.log(`${n} files, ${(total / 1e6).toFixed(2)} MB`)
}
