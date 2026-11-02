import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { Resvg } from '@resvg/resvg-js'
import { render } from './render.tsx'

const [outDir, dest] = process.argv.slice(2)
const ROSTER = [['Jojo', 'jojo'], ['Tata', 'o03'], ['Pocket', 'o05'], ['Forge', 'o08'], ['Watch', 'o11'], ['Scout', 'o07'], ['Ledger', 'h02'], ['Taro', 'h03']]
const STATUSES = ['idle', 'working', 'needs-input', 'success', 'error', 'offline']
const SIZES = [24, 32, 96]
const COL = 24 + 8 + 32 + 8 + 96 + 24, ROW = 120, LEFT = 90, TOP = 40
const panelW = LEFT + STATUSES.length * COL, panelH = TOP + ROSTER.length * ROW
let uid = 0
// each avatar is its own image in the app; in one sheet their defs ids must not collide
function nested(svg: string, x: number, y: number) { const u = `i${uid++}`; return svg.replaceAll('jojo-_R_0_', `jojo-${u}`).replace('<svg ', `<svg x="${x}" y="${y}" `) }
let body = ''
for (const [pi, [bg, fg]] of [['#ffffff', '#1d1d1f'], ['#1e1e20', '#f2f2f7']].entries()) {
  const ox = pi * panelW
  body += `<rect x="${ox}" y="0" width="${panelW}" height="${panelH}" fill="${bg}"/>`
  STATUSES.forEach((s, si) => { body += `<text x="${ox + LEFT + si * COL}" y="24" font-family="Helvetica" font-size="13" fill="${fg}">${s}</text>` })
  ROSTER.forEach(([name, id], ri) => {
    const y = TOP + ri * ROW
    body += `<text x="${ox + 10}" y="${y + 56}" font-family="Helvetica" font-size="13" fill="${fg}">${name}</text><text x="${ox + 10}" y="${y + 72}" font-family="Helvetica" font-size="11" fill="${fg}" opacity="0.6">${id}</text>`
    STATUSES.forEach((s, si) => {
      let x = ox + LEFT + si * COL
      for (const px of SIZES) {
        // the bundled files for the sizes the app ships, a direct render for 96
        const svg = px === 96 ? render(id, s, 96) : readFileSync(join(outDir, id, `${s}-${px}.svg`), 'utf8').trim()
        body += nested(svg, x, y + (96 - px) / 2 + 8)
        x += px + 8
      }
    })
  })
}
const sheet = `<svg xmlns="http://www.w3.org/2000/svg" width="${panelW * 2}" height="${panelH}">${body}</svg>`
const png = new Resvg(sheet, { fitTo: { mode: 'zoom', value: 2 }, font: { loadSystemFonts: true } }).render().asPng()
writeFileSync(dest, png)
console.log(dest, panelW * 2, panelH)
