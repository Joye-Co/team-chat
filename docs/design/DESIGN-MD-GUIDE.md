# How to write a production DESIGN.md

This guide says what a production-grade design document contains, based on the DESIGN.md files and
design systems of maintained open-source products. Use it to review and extend
[docs/DESIGN.md](../DESIGN.md). It sets no visual values of its own: DESIGN.md stays the only
source of values. Where this guide quotes a measurement, it is evidence from another product, not
a rule for us.

All text below is our summary. Nothing is copied from the sources. Several sources are AGPL, MPL
or have mixed licenses: take measurements and ideas only, never code, styles or assets (the same
rule as OpenBot in DESIGN.md).

Units: web `px` at 1x equals macOS `pt`. A web value of 40 px means a 40 pt control in our app.

## 1. Sources

Read on 2026-11-03. "Read in" says where the evidence came from.

| # | Source | What it is | Why it is here | Read in |
| --- | --- | --- | --- | --- |
| 1 | [google-labs-code/design.md](https://github.com/google-labs-code/design.md) ([spec](https://github.com/google-labs-code/design.md/blob/main/docs/spec.md), [philosophy](https://github.com/google-labs-code/design.md/blob/main/PHILOSOPHY.md)) | The DESIGN.md format for coding agents (Google Stitch), Apache-2.0, about 29k stars | The convention itself: token front matter + prose, fixed section order, a linter | README, PHILOSOPHY.md |
| 2 | [Atlassian DESIGN.md](https://atlassian.design/DESIGN.md) | First-party DESIGN.md of the Jira and Confluence system | Most complete first-party file: tokens, components, voice, accessibility, motion, responsive | Fetched page |
| 3 | [Oxide design.md](https://github.com/oxidecomputer/design-system/blob/master/design.md) | First-party file of Oxide's design system, MPL-2.0 | Best on states, motion rules, elevation and a strict Do and Don't list | Full file |
| 4 | [Ant Design design.md](https://ant.design/design.md) | First-party file of Ant Design | Type scale with line heights, one control height, motion tiers, "no magic numbers" rules | Fetched page |
| 5 | [Vercel design.md](https://vercel.com/design.md) | First-party file of Geist (report pages) | Shows the prose side: priority order, banned "generated design" habits, review passes | Fetched page |
| 6 | [Primer primitives size tokens](https://github.com/primer/primitives/blob/main/src/tokens/functional/size/size.json5) | GitHub's token source | Each token carries its own `usage` and `rules`: documentation that lives in the token file | Full file |
| 7 | [Zulip web styles](https://github.com/zulip/zulip/tree/main/web/styles) + [contributor docs](https://github.com/zulip/zulip/tree/main/docs/contributing) | Topic-based team chat, Apache-2.0 | Topics as the unit (our 话题), density settings, comments that state every value in px | `app_variables.css`, `compose.css`, `left_sidebar.css`, `inbox.css`, `zerver/models/users.py`, `presenting-visual-changes.md`, `html-css.md` |
| 8 | [Mattermost webapp styles](https://github.com/mattermost/mattermost/tree/master/webapp/channels/src/sass) (Compass) | Team chat; Compass is its design language | Composer, buttons, sidebar rows; an agent file for its styles folder | `advanced_text_editor.scss`, `wysiwyg_editor.scss`, `_buttons.scss`, `_css_variables.scss`, `_sidebar-left.scss`, `sass/CLAUDE.OPTIONAL.md` |
| 9 | [Rocket.Chat Fuselage](https://github.com/RocketChat/fuselage) | Rocket.Chat's design system, MIT | A token package with a named type scale, input and button sizes, message components | `fuselage-tokens/src/typography.json`, `InputBox`, `Button`, `Message`, `SidebarItem` styles |
| 10 | [Element Compound](https://github.com/element-hq/compound-web) + [tokens](https://github.com/element-hq/compound-design-tokens) + [element-web](https://github.com/element-hq/element-web) | Matrix client design system, AGPL-3.0 | One token source built for web, iOS and Android; field states; room list and composer | `platform-web.json`, `cross-platform.json`, `Button.module.css`, `Text.module.css`, `_MessageComposer.pcss`, `_SendMessageComposer.pcss`, `_EventTile.pcss`, `RoomListItemView.module.css` |
| 11 | [Zed ui crate](https://github.com/zed-industries/zed/tree/main/crates/ui/src/styles) + [UI checklist](https://github.com/zed-industries/zed/blob/main/docs/src/development/ui-checklist.md) | The GPUI reference app | How a GPUI app keeps the design system in code: size enums, density-aware spacing, elevation levels, component previews | `typography.rs`, `spacing.rs`, `elevation.rs`, `button_like.rs`, crate list (`component`, `component_preview`) |
| 12 | [VoltAgent/official-design-md](https://github.com/VoltAgent/official-design-md) | Index of first-party DESIGN.md files | Where sources 2 to 5 were found (also lists Clerk, Mintlify, Nuxt, Resend, Microsoft FAST, Juspay) | README |

Not used: the many "awesome DESIGN.md" collections (VoltAgent/awesome-design-md and others). They
are files extracted from websites by third parties, not documents that a product team maintains.
GitHub code search for `filename:DESIGN.md` returns mostly software architecture notes, not design
systems.

## 2. What the best files have in common

### 2.1 The format (source 1)

- **Two layers.** Machine-readable tokens (YAML front matter or YAML blocks in each section) give
  exact values. Prose gives the reason and the rule of use. The tokens are normative; the prose
  says when to use which token.
- **A fixed section order**: Overview, Colors, Typography, Layout, Elevation and Depth, Shapes,
  Components, Do's and Don'ts. A file may omit a section, but the order stays. Unknown sections
  are kept.
- **Components refer to tokens** (`{colors.primary}`), never to raw values. Each state (hover,
  pressed) is its own entry with a related name.
- **A linter checks the file**: broken token references, contrast of every text and fill pair
  below WCAG AA, tokens that no component uses, sections out of order. A `diff` command reports
  token changes between two versions. An `export` command writes the tokens for other tools.
- **Name a specific reference, not adjectives.** The philosophy says one concrete reference
  ("a printed graduate lecture handout") carries more than "clean, modern, premium". Our
  equivalent is "Slack, Feishu 话题群 and GrokBot side by side". A short, intentional Do and Don't
  list then states the limits.

### 2.2 Sections and how rigorous each one is

For each section: what the strong files do, then where to look.

**Principles and overview.** Three to seven principles, each one a decision with a consequence,
not a value word. Vercel ranks its concerns in a priority order, so a conflict has an answer.
Oxide states its character in one line and repeats it in rules ("feedback is quiet").
Examples: [Vercel](https://vercel.com/design.md) "priority order", [Atlassian](https://atlassian.design/DESIGN.md) Overview.

**Tokens with roles.** Every color is a semantic role (surface, content, stroke), never a hue
name in product code. Oxide has three families (`bg-*`, `text-*`, `border-*`) and gives text four
tiers (default, secondary, tertiary, quaternary). It forbids opacity to fake a tier, because the
tiers are tuned per theme. Atlassian separates neutrals, semantic roles, accent ramps and chart
colors, and says which kind is allowed where (success uses the semantic token, not a green ramp
step). Primer puts `usage` and `rules` inside each token.
Examples: [Oxide](https://github.com/oxidecomputer/design-system/blob/master/design.md) "The three token layers", [Primer size.json5](https://github.com/primer/primitives/blob/main/src/tokens/functional/size/size.json5).

**Type scale.** A closed list of named styles, each with size, line height and weight together
(never a size alone). Ant: body 14/22, small 12/20, title 16/24. Fuselage: `p2` 14/20,
`p1` 16/24, `c1` 12/16, `h5` 14/20 bold. Compound: body 15 at 150 %, small 13, extra small 11.
Zulip: 16 at 140 % by default, 14 at 122 % in compact mode. Zed: 16, 14, 12, 10.
Most files also state the weights that exist (Ant and Oxide: two) and case rules (Atlassian:
sentence case, no all caps).
Examples: [Fuselage typography.json](https://github.com/RocketChat/fuselage/blob/main/packages/fuselage-tokens/src/typography.json), [Compound platform-web.json](https://github.com/element-hq/compound-design-tokens/blob/main/tokens/platform-web.json).

**Spacing and layout.** A 4 pt base scale with named steps; a rule for gaps ("group with 8,
separate with 16", Oxide); one control height that sets the rhythm so controls align side by
side; a reading measure; reference window widths to test. Ant bans magic-number spacing.
Atlassian lists the drift it rejects (`padding: 13px`).
Examples: Oxide "Layout & Spacing", Ant "Layout", [Compound cross-platform.json](https://github.com/element-hq/compound-design-tokens/blob/main/tokens/cross-platform.json) (`space.1x` = 4).

**Color usage rules.** Color is rationed and each color has one meaning. Oxide: status hues
appear only for their meaning, never as decoration; no gradients except the scrim. Vercel:
monochrome by default, color only for meaning and always with a second cue (icon, label, shape).
Every file says "never color alone".

**Components.** The strong files give each component:
- anatomy (parts and their order);
- sizes as a short named set with height, padding, text style and gap (Primer: 24, 28, 32, 40,
  48; Fuselage buttons 28, 32, 40; Mattermost buttons 24, 32, 40, 48);
- every state: rest, hover, pressed, focus-visible, selected, disabled, loading, error, read only
  (Compound text field defines all of them, including a hover shadow and a read-only fill);
- variants with when to use each (Oxide button: primary, secondary, ghost, danger);
- behavior: what a loading or disabled control does, not only how it looks (Oxide: disabled and
  loading also block input and set the accessible state);
- do and don't.
Examples: Oxide "Components", Atlassian "Components" (modal widths, toggle and checkbox sizes,
tooltip widths), [Compound Text.module.css](https://github.com/element-hq/compound-web/blob/main/src/components/Form/Controls/Text/Text.module.css).

**Content and copy.** Voice rules with examples of right and wrong: imperative button labels,
errors that say the reason and the fix, descriptive links, sentence case (Atlassian "Voice and
tone"). Zed's checklist asks for concise strings with no insider words. Ours: PRODUCT.md words
and Chinese punctuation rules.

**Accessibility.** WCAG 2.2 AA as the stated floor (Atlassian), with contrast computed per text
and fill pair (the Google linter does it automatically). A visible focus ring with exact width
and offset (Atlassian 2 px with a 2 px offset; Oxide 2 px). Visible labels, full keyboard use,
no color-only meaning, zoom to 200 %. Hit targets: 44 pt for touch (Oxide, Primer), at least 32
pt otherwise (Atlassian). Oxide extends the hit area instead of enlarging the control.

**Motion.** Duration tiers and named curves, with a reduced-motion rule for each item. Atlassian:
50 to 150 ms for interactions, 150 to 400 ms for transitions, no bounce. Ant: 100, 200, 300 ms.
Oxide: 150 to 300 ms; ease-out to enter and exit, ease-in-out to move, never ease-in; the more
often a control is used, the less it moves; enter from 0.95 scale, not 0; a spinner stays at
least 500 ms so it does not flicker. Skeletons for content, spinners for actions.

**Dark mode.** The best files make dark mode a property of the tokens: the same semantic token
resolves in both themes, and product code never writes a light or dark literal (Oxide, Ant's
dark algorithm, Compound's separate theme files including high-contrast light and dark). Weak
spot in most files: Atlassian and Ant describe only light values in the DESIGN.md itself.

**Density.** Zulip lets each user set font size (12 to 20) and line spacing; the default is
16/140 % and compact is 14/122 %. Zed derives every spacing step for three densities from one
table (`DynamicSpacing::Base16` is 14, 16 or 18). Ant composes a compact algorithm. Rule shared by
all: density changes spacing and type through tokens, never through view code.

**Examples.** Code or markup for each component (Oxide), a Do and Don't table of real drift
(Atlassian), reference screenshots. Zulip requires precise before and after screenshots of every
affected state in each visual pull request, and screenshots that prove "no visual change" when
that is the claim ([presenting-visual-changes.md](https://github.com/zulip/zulip/blob/main/docs/contributing/presenting-visual-changes.md)).

**Staying in sync with code.** This is the weakest part of most files. What works:
- One token source that generates the platform files (Compound builds web, iOS and Android from
  one set of JSON tokens; the Google CLI exports to DTCG and Tailwind).
- Rules inside the tokens (Primer) so documentation and values cannot drift apart.
- A linter or lint plugin in CI (Google `design.md lint`, Atlassian ESLint plugins).
- Code that only accepts named steps: Zed's `TextSize`, `ButtonSize`, `DynamicSpacing` and
  `ElevationIndex` enums make an off-scale value a compile error, and the `component_preview`
  crate renders each component in all states.
- Comments that state the real value next to each relative one (Zulip writes "28px at 14px/1em"
  beside every `em` value).
- An agent file next to the styles (Mattermost `sass/CLAUDE.OPTIONAL.md`: always use the CSS
  variables, use the mixins for breakpoints).

## 3. Checklist for our DESIGN.md

Our DESIGN.md passes when every line is true. Each line can be checked by reading the file or
the code.

Format and sync
- [ ] Sections follow the order: principles, color, type, spacing and layout, elevation, shape,
      components, content, accessibility, motion, dark mode, density, worst-case data, examples,
      checklist. Screen-by-screen rules come after the components they use.
- [ ] Every value has one home in tokens (`tokens.mjs` for mockups, `theme.json` plus
      `theme::Colors` for the app), and DESIGN.md names the token, not only the number.
- [ ] Sizes are Rust enums or constants in `ui/`, so an off-scale size does not compile (Zed
      pattern). Views never write a color, size or duration literal.
- [ ] A check compares DESIGN.md tables with `tokens.mjs` and `theme.json` (values and contrast),
      in the style of `design.md lint`.
- [ ] Every component has a preview screen (or self-test screenshot) that shows all its states in
      both themes.
- [ ] Each change to a value updates DESIGN.md, the tokens and the mockups in one commit.

Principles and references
- [ ] Principles are decisions with consequences, ranked for conflicts.
- [ ] Each screen names the reference product it follows and what we rejected.

Tokens and color
- [ ] Colors are semantic roles in families (surface, text, line, fill, accent, status); no hue
      names in views; text tiers are tokens, never opacity.
- [ ] Contrast is computed for every text and fill pair over its named surface, in both themes,
      and meets 4.5:1 for content text and 3:1 for large text, icons and focus rings.
- [ ] Each color has one meaning, and every status has a second cue (glyph or word).

Type
- [ ] A closed set of named styles, each with size, line height and weight.
- [ ] Body is 14/22. List titles at least 14. Meta at least 12. Nothing a reader must read is
      below 12 (11 only for number badges and kbd).
- [ ] Weights, numerals (tabular), truncation and CJK punctuation are stated.

Spacing, layout and sizes
- [ ] 4 pt base scale with named steps and a grouping rule.
- [ ] One control height table: buttons 32 to 36; search and filter fields 36; form fields 40;
      icon buttons hit area at least 28 with a 32 target where space allows; rows per list.
- [ ] Reading measure, column widths (min, default, max) and test window sizes.

Components (for each one)
- [ ] Anatomy, sizes, padding, radius, text style, icon size.
- [ ] Every state: rest, hover, pressed, focus-visible, selected, disabled, loading, error, read
      only, empty, with the token for each.
- [ ] Behavior: keyboard, what disabled and loading block, what happens on error.
- [ ] Do and don't, with the worst-case data it must survive.

Composer (hard requirement from Joye)
- [ ] Empty height at least 96 pt: two visible lines of 14/22 plus 12 pt padding, and a toolbar
      row of about 36 pt (attach, @, emoji and format at the left; send at the right).
- [ ] Radius 12; grows with the text to 40 % of the topic view, then scrolls inside.
- [ ] Clear focus state (border change visible at a glance in both themes), hover state,
      disabled and blocked states.

Content
- [ ] Interface words come only from PRODUCT.md; error text says what failed and what to do;
      placeholders name the destination.

Accessibility
- [ ] Focus ring width, offset and color; full keyboard paths for every screen; no color-only
      meaning; reduced motion and increased contrast behavior.

Motion
- [ ] Duration tiers and curves; frequency rule (frequent actions do not animate); a minimum
      show time and a delay for busy indicators; a reduced-motion rule per item.

Dark mode and density
- [ ] Every token has a dark value; elevation steps are visibly distinct in dark.
- [ ] Density is a token-level setting (or explicitly out of scope), never a per-view change.

Examples and acceptance
- [ ] Mockups for each screen in both themes, and a before and after capture for each visual
      change, at the same scale as the references.
- [ ] A per-change checklist that a reviewer can run screen by screen.

## 4. Patterns from open-source chat apps

Measured in the source code on 2026-11-03. Values are px at the product's default settings.

### 4.1 Composer

| Product | Empty height | Text | Grows to | Border and focus | Toolbar | Source |
| --- | --- | --- | --- | --- | --- | --- |
| Mattermost | 98 (the loading skeleton reserves this height) | Body 14 | Text area 46 min, 560 max (rich editor) | 2 px line at 16 % text color; hover 24 %; focus 32 %; radius 4 | Action buttons 32 × 32, radius 4; format bar 48 high when shown; 24 side margin, 24 below, 12 gap | [advanced_text_editor.scss](https://github.com/mattermost/mattermost/blob/master/webapp/channels/src/components/advanced_text_editor/advanced_text_editor.scss), [wysiwyg_editor.scss](https://github.com/mattermost/mattermost/blob/master/webapp/channels/src/components/advanced_text_editor/wysiwyg_editor/wysiwyg_editor.scss) |
| Zulip | Recipient row 32 + text area + format row 28 | 16/140 % | Expand button to full height | Banners above the box, max min(25vh, 240) | Format row 28 high (2em at 14); send area 112 wide | [app_variables.css](https://github.com/zulip/zulip/blob/main/web/styles/app_variables.css), [compose.css](https://github.com/zulip/zulip/blob/main/web/styles/compose.css) |
| Element | Bar 60 min; input 55 min | Body 15 | 140 max, then scrolls | The text area has no border or outline of its own | Icons beside a single line | [_MessageComposer.pcss](https://github.com/element-hq/element-web/blob/develop/apps/web/res/css/views/rooms/_MessageComposer.pcss), [_SendMessageComposer.pcss](https://github.com/element-hq/element-web/blob/develop/apps/web/res/css/views/rooms/_SendMessageComposer.pcss) |

Lesson: the team-chat products that write long messages (Mattermost, Zulip) reserve about 100 px
for an empty composer with a separate toolbar row, a visible border that strengthens on hover and
focus, and 32 px action buttons. Element's compact single-line bar is the counter-example: it
suits 1:1 chat, not topic replies. This supports Joye's 96 pt minimum.

### 4.2 Message rows

| Product | Avatar / gutter | Body | Row padding | Source |
| --- | --- | --- | --- | --- |
| Zulip | Avatar 2.5em (35 at 14, 40 at 16); avatar column 46 at 14 | 16/140 %; message area max 918.6 px | Gap between Markdown blocks 5 | [app_variables.css](https://github.com/zulip/zulip/blob/main/web/styles/app_variables.css) |
| Mattermost | Post action buttons 36; avatar 32 | 14, header lines 20 | 8 top, 1.5em left | [_post.scss](https://github.com/mattermost/mattermost/blob/master/webapp/channels/src/sass/components/_post.scss) |
| Rocket.Chat | Avatar column 36 | `p2` 14/20; name 14/20 bold; time `c1` 12/16 | 8 above, 4 below, 20 at the sides | [Messages.styles.scss](https://github.com/RocketChat/fuselage/blob/main/packages/fuselage/src/components/Message/Messages.styles.scss) |
| Element | Left gutter 64 | 15/150 %; grouped lines 22 | 1 above, 3 below in a group | [_EventTile.pcss](https://github.com/element-hq/element-web/blob/develop/apps/web/res/css/views/rooms/_EventTile.pcss) |

Lesson: nobody sets message text below 14; line height is 20 to 24; the avatar is 32 to 40 with a
gutter of 36 to 64; the reading width is capped.

### 4.3 Sidebars and topic lists

| Product | Row height | Text | Notes | Source |
| --- | --- | --- | --- | --- |
| Mattermost | Channel row 32; sidebar header 55 | 14 | Unread by weight; sections collapse | [_sidebar-left.scss](https://github.com/mattermost/mattermost/blob/master/webapp/channels/src/sass/layout/_sidebar-left.scss) |
| Zulip | Line height 22 (row) and 24 (prominent row) at 14; topics nested under the channel | 14 to 16 | Inbox rows 30 min; search box 32 | [left_sidebar.css](https://github.com/zulip/zulip/blob/main/web/styles/left_sidebar.css), [inbox.css](https://github.com/zulip/zulip/blob/main/web/styles/inbox.css) |
| Element | Room row 52 (44 + 4 + 4), two lines | Name 15, preview 13 | Hover menu replaces the badge on hover; keyboard focus shows the same menu | [RoomListItemView.module.css](https://github.com/element-hq/element-web/blob/develop/packages/shared-components/src/room-list/VirtualizedRoomListView/RoomListItemWrapper/RoomListItemView/RoomListItemView.module.css) |
| Rocket.Chat | Item padding 4 (8 in the larger size) by 16; avatar 20 | `p2` 14/20; badge `c1` 12 | | [SidebarItem.styles.scss](https://github.com/RocketChat/fuselage/blob/main/packages/fuselage/src/components/Sidebar/SidebarItem/SidebarItem.styles.scss) |

Lesson: one-line navigation rows are 28 to 32 with 14 text; two-line rows with a preview are
about 52 with a 15 title and a 13 preview. Hover actions replace trailing badges instead of
adding width.

### 4.4 Fields and buttons

| Product | Field | Buttons | Source |
| --- | --- | --- | --- |
| Rocket.Chat | 40 min, 14/20 text, 8 by 16 padding | 28, 32, 40 | [InputBox.styles.scss](https://github.com/RocketChat/fuselage/blob/main/packages/fuselage/src/components/InputBox/InputBox.styles.scss), [Button.styles.scss](https://github.com/RocketChat/fuselage/blob/main/packages/fuselage/src/components/Button/Button.styles.scss) |
| Element | About 48 (15 text, 12 by 16 padding, 1 px line), radius 8; hover line + small shadow; focus 2 px ring; error, disabled and read-only states | 36 small, 48 large, pill | [Text.module.css](https://github.com/element-hq/compound-web/blob/main/src/components/Form/Controls/Text/Text.module.css), [Button.module.css](https://github.com/element-hq/compound-web/blob/main/src/components/Button/Button.module.css) |
| Mattermost | — | 24, 32, 40 (default, 14 text, 20 side padding), 48 | [_buttons.scss](https://github.com/mattermost/mattermost/blob/master/webapp/channels/src/sass/components/_buttons.scss) |
| Primer | Same sizes as buttons | 24, 28, 32 (default), 40, 48; 44 target on touch | [size.json5](https://github.com/primer/primitives/blob/main/src/tokens/functional/size/size.json5) |
| Oxide | 40 base, 32 compact | Same | [design.md](https://github.com/oxidecomputer/design-system/blob/master/design.md) |
| Zed (IDE) | — | 16, 18, 22 (default), 28, 32 | [button_like.rs](https://github.com/zed-industries/zed/blob/main/crates/ui/src/components/button/button_like.rs) |

Lesson: chat and product apps use 32 to 40 for default controls and 40 to 48 for form fields.
Zed's 22 pt default is IDE density; copy Zed's code structure, not its sizes.

## 5. Where docs/DESIGN.md v2 does not pass yet

Found by checking the v2 file against section 3. This guide does not change DESIGN.md; the design
owner decides each item.

- §5 sets 28 pt standard controls and 30 pt fields in the sidebar and settings, and §11.6 sets
  36 pt on login. Joye's requirement and the sources (section 4.4) say 32 to 36 for buttons,
  36 for search and filters, 40 for form fields.
- §10 composer is a single row with padding 9/8/8/12 and a 28 pt send button, which gives an
  empty box of about 40 pt. The requirement is at least 96 pt with a toolbar row. The focus state
  (1 pt line + 3 pt soft ring, "no blue") is weaker than Mattermost's or Element's.
- §4 `ui` 13/18 is used for topic row titles and sidebar rows. The requirement is list titles of
  at least 14.
- No component has an anatomy and full state table of its own; §7 gives one shared state table.
  Loading, error and read-only states are missing for fields.
- No check keeps DESIGN.md, `tokens.mjs` and `theme.json` in step, and sizes are not enums yet.
- Dark mode values exist, but §16 says they were never checked on a real display.
- Density is not mentioned.
- Accessibility has a focus ring and contrast values but no keyboard map per screen and no
  increased-contrast behavior.

## 6. Not confirmed

- Mattermost avatar size in posts (32) and post text size (14): taken from class names and nearby
  rules in `_post.scss`, not from one clear declaration.
- Element composer frame: the text area has no border; the styles of the outer frame were not
  read.
- Zulip's text area minimum height: not found in `compose.css`; its total empty composer height
  was not measured.
- Atlassian, Ant and Vercel files were read through a page summary tool, not as raw text.
- Slack, Feishu and GrokBot are not open source, so they are not in section 4. Their evidence is in
  `~/.cache/team-chat/design/refs/` as DESIGN.md says.
