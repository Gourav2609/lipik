# Lipik design system

Lipik (लिपिक, "scribe") is a desktop dictation app: hold a key, speak, and clean text appears where you are typing. This document is the single source for how Lipik looks, moves and talks. Tokens live in `src/flow/flow.css`; shared components live in `src/flow/ui`.

## 1. Concept

**A scribe with a ruled page.** Lipik listens, then writes in clean ink: quiet paper, one indigo ink, hairline rules like an exercise book, and a single ink line in the margin for the thing that matters.

Calm and precise rather than playful. Made in India without the usual signs (no saffron, paisley, mandalas or flags): the Indian part is in the name, in the type (both faces carry Devanagari) and in the _shirorekha_, the headline stroke that Devanagari letters hang from, which is the basis of the logo and of the recording waveform.

Rules that follow from the concept:

- One accent colour (ink). Red appears only for "live" (recording) and for destructive actions.
- Lines, not boxes. Lists are ruled lines; cards are flat tinted areas or raised white sheets, never heavy outlined boxes.
- No photographs, gradients or illustrated characters in the product. Art, where needed, is line drawing in ink.
- No italic emphasis. Emphasis is ink colour.

## 2. Colour

All colours are CSS custom properties on `.fk` (main window). Use the token, never the hex value.

| Token                                   | Light                 | Dark                  | Use                                                        |
| --------------------------------------- | --------------------- | --------------------- | ---------------------------------------------------------- |
| `--fk-bg`                               | `#eceff4`             | `#0f121a`             | Window and sidebar ground                                  |
| `--fk-panel`                            | `#fbfcfd`             | `#151923`             | Page (content area)                                        |
| `--fk-surface`                          | `#f2f4f8`             | `#1b202c`             | Flat cards, inputs, row hover                              |
| `--fk-surface-2`                        | `#e8ebf1`             | `#222836`             | Secondary buttons, tab track, hint card                    |
| `--fk-menu-bg`                          | `#ffffff`             | `#212634`             | Raised sheets: menus, dialogs, active nav item, active tab |
| `--fk-border`                           | `#dadfe7`             | `#2a3040`             | Hairlines and rules                                        |
| `--fk-border-strong`                    | `#c3c9d5`             | `#3a4256`             | Key caps, scrollbars, input borders on hover               |
| `--fk-hover`                            | `#e6e9f0`             | `#222838`             | Hover on transparent controls                              |
| `--fk-active`                           | `#dfe4f6`             | `#262e4e`             | Pressed/selected (ink tint)                                |
| `--fk-text`                             | `#141826`             | `#e7eaf3`             | Body text (blue-black ink)                                 |
| `--fk-text-muted`                       | `#596175`             | `#9aa2b6`             | Secondary text, icons at rest                              |
| `--fk-text-faint`                       | `#646c80`             | `#868ea4`             | Placeholders, timestamps, counts                           |
| `--fk-accent`                           | `#3245c4`             | `#8d9cff`             | Ink: primary buttons, links, active marks, focus           |
| `--fk-accent-hover`                     | `#2a3aa8`             | `#a5b1ff`             | Primary button hover                                       |
| `--fk-accent-soft`                      | `#e3e7fb`             | `#232a4d`             | Margin notes, soft buttons, progress track                 |
| `--fk-accent-text`                      | `#ffffff`             | `#0d1024`             | Text on ink                                                |
| `--fk-live` / `--fk-live-soft`          | `#c23346` / `#fbe5e8` | `#ff6b7d` / `#3a1c25` | Recording, destructive                                     |
| `--fk-ok` / `--fk-ok-soft`              | `#276b44` / `#e1f1e7` | `#6fcf97` / `#173326` | Success, "connected"                                       |
| `--fk-toggle-off`                       | `#7f879b`             | `#646d86`             | Toggle track when off (knob `#ffffff` / `#e7eaf3`)         |
| `--fk-inverse-bg` / `--fk-inverse-text` | `#141826` / `#f4f6fb` | `#eef1f8` / `#141826` | Tooltips, toasts, small badges                             |
| `--fk-scrim`                            | `rgba(20,24,38,.32)`  | `rgba(5,7,12,.6)`     | Behind dialogs                                             |

Older names (`--fk-checklist-bg`, `--fk-check-*`, `--fk-progress-*`, `--fk-toggle-*`, `--fk-btn-secondary*`, `--fk-modal-*`, `--fk-shadow`) still exist as aliases of the tokens above so existing code keeps working. New code uses the names in the table.

Fixed brand colours for places outside the themed window (pill, app icon, tray) are exported from `src/flow/brand.ts` as `BRAND`: ink `#285a44`, inkLight `#9cc8ae`, night `#12110f`, paper `#f6f2ea`, live `#d9574b`. The app icon and every menu-bar icon are drawn from the same mark by `scripts/make-icons.mjs`.

Contrast: text, muted and faint text meet WCAG AA (4.5:1) on `--fk-panel`, `--fk-bg` and `--fk-surface` in both themes; `--fk-live` and `--fk-ok` meet it on their soft fills; the off toggle track meets 3:1 against the page. Dark `--fk-menu-bg` sits one step above `--fk-surface`, so inputs inside dialogs stay visible. Check new pairs before adding them.

## 3. Type

| Role                               | Face   | Size / line      | Weight                         |
| ---------------------------------- | ------ | ---------------- | ------------------------------ |
| Display: page title                | Martel | 28 / 38          | 400                            |
| Display: margin note, dialog title | Martel | 22 / 30          | 400                            |
| Display: big numbers (stats)       | Martel | 32 / 36          | 400, `fk-num`                  |
| Wordmark                           | Martel | 18               | 600                            |
| Section title                      | Mukta  | 16 / 22          | 600                            |
| Body                               | Mukta  | 15 / 22          | 400                            |
| Controls (nav, buttons, tabs)      | Mukta  | 14–14.5 / 1      | 500 (nav, tabs), 600 (buttons) |
| Small                              | Mukta  | 13 / 18          | 400                            |
| Label (`fk-eyebrow`)               | Mukta  | 12 / 16, +0.02em | 600, muted, sentence case      |

- **Mukta** (UI sans, Ek Type, Mumbai) and **Martel** (display serif, Dan Reynolds) are both SIL Open Font License, loaded from Fontsource (`@fontsource/mukta`, `@fontsource/martel`). Both include Devanagari, so Hindi text never falls back to a system font.
- Use `.fk-display` for Martel (`.fk-serif` is an alias kept for old code). Display text is never italic and never tracked tighter than `-0.005em`.
- Labels are sentence case ("Yesterday", "This week"), never all caps.
- Times, counts and stats use `.fk-num` (tabular figures).

## 4. Space, radius, elevation

**Spacing scale (px):** 2, 4, 8, 12, 16, 20, 24, 32, 40, 48, 64 for layout: gaps between blocks, card and page padding, section spacing. Inside a control (icon-to-label gaps, chip and menu-item padding) 6 and 10 are also allowed.

**Radii:** `--fk-r-xs` 4 (badges), `--fk-r-sm` 8 (small buttons, icon buttons, rows, tabs), `--fk-r-md` 10 (buttons, inputs, tab track, pill tile), `--fk-r-lg` 14 (cards, menus, margin notes), `--fk-r-xl` 20 (dialogs). Full round only for toggles, avatars and dots.

**Elevation:**

- `--fk-elev-1`: a 1 px ring plus a soft 2 px shadow. Raised sheets that sit on the page: active nav item, active tab, `Card tone="panel"`.
- `--fk-elev-2`: menus, popovers, the pill's panels.
- `--fk-elev-3`: dialogs and the settings window.

Flat areas (`--fk-surface`) carry no shadow and no border.

## 5. Icons

- Lucide only (`lucide-react`), stroke width **1.75**, size **18** in navigation and toolbars, **16** inside rows and buttons, **14** inside small chips.
- Icons at rest use `--fk-text-muted`; active or selected icons use `--fk-accent`.
- No filled icons, no emoji, no sparkle marks for "AI". Auto-learned items get a small ink dot, not a sparkle.
- Navigation icons: Home `House`, Meetings `NotebookPen`, Drafts `NotepadText`, Vocabulary `BookA`, Snippets `TextQuote`, Tone `Signature`, Rewrite `PencilLine`, Stats `ChartNoAxesColumn`, Settings `Settings2`, Help `LifeBuoy`.

## 6. Main window layout

```
┌────────────────────┬────────────────────────────────────────────┐
│ ● ● ●  ╤╤╤ Lipik   │ [▯]                       theme  bell  me  │  52 px header row
│                    │                                            │
│ ▌Home              │  Page title (Martel 28)         [actions]  │
│  Meetings          │  One muted line                            │
│  Drafts            │  ────────────────────────────────────────  │  ruled page head
│  Vocabulary        │                                            │
│  …                 │  content, max 1080 wide, 48 px margins     │
│                    │                                            │
│ [Hold fn and speak]│                                            │
│ ────────────────── │                                            │
│  Settings          │                                            │
│  Help              │                                            │
└────────────────────┴────────────────────────────────────────────┘
       232 px                  page (flush, no inset card)
```

- **Window:** two flush columns. The sidebar sits on `--fk-bg`; the page fills the rest on `--fk-panel`, separated by one hairline. The page is not a floating rounded card.
- **Header row:** 52 px tall across the whole window. The macOS window buttons sit 16 px from the left, centred in it; the brand (mark + wordmark) sits in the same row at x = 96 (x = 20 in full screen). Over the page, the row holds the sidebar toggle on the left and Appearance, Notifications and Account on the right (30 px icon buttons, 4 px apart, 12 px from the edges). The native window-button placement must use a 52 pt bar.
- **Sidebar (232 px):** nav items 34 px tall, radius 8, 2 px apart, icon 18 + label 14.5/500. The active item is a raised white sheet (`--fk-menu-bg` + `--fk-elev-1`) with a 2 px ink line in its left margin; inactive items are muted text on nothing. At the bottom: the "Hold fn and speak" hint card (`--fk-surface-2`, radius 10), a hairline, then Settings and Help.
- **Rail (80 px):** the collapsed sidebar is a rail, not icons alone: each item is 52 px tall with the icon above a 10.5 px label. The logo mark sits under the window buttons. Width changes over `--fk-t-layout` with `--fk-ease-move`; the contents fade in again after the width settles.
- **Page grid:** `.fk-page` = max width 1080, padding 8 / 48 / 48, centred. Two-column pages use `1fr 280px` with a 32 px gap; the right column is for summaries (stats, help), never for primary actions.
- **Page head:** `<PageHeader title description actions />`: Martel 28/38 title, one muted line, actions on the right, a hairline under it and 28 px before the content. Every page starts with one. Home uses it for the greeting, with the three totals (words, words a minute, days in a row) as one compact line in the actions slot that opens Stats; Home has no right column, and its history list takes the full width.
- **Sections:** 40 px apart. Section title Mukta 16/600, optional count in `--fk-text-faint`, actions right-aligned on the same line.
- **Cards:** `Card tone="surface"` (flat tint) for grouped settings and stats; `Card tone="panel"` (raised white sheet) for things you can pick or open. Padding 20, radius 14.
- **Lists:** `ListGroup` + `ListRow` are ruled lines: a hairline above the first row and under each row, no outer box, rows 48 px min, hover tints the whole line, row actions appear on hover or focus.
- **Margin note:** `HeroBanner` is a tinted ink card (`--fk-accent-soft`) with a 3 px ink rule down its left edge, a Martel 22 title, one muted sentence and at most two buttons (`HeroButton strong` for the main one). It explains a page the first time; it can be dismissed. The title is plain text, with no word picked out in ink. No photos or dark banners. Tone and Stats have no margin note: their content shows straight away.
- **Tabs:** `SectionTabs` is a segmented control: a `--fk-surface-2` track (36 px, radius 10, 3 px inset) with the active tab as a raised white sheet.
- **Inputs:** 38 px, radius 10, `--fk-surface` fill with no border at rest, a hairline on hover, ink border plus a 3 px soft ink ring on focus. The ring sits on the field, never on the bare `<input>` inside it. On a flat `--fk-surface` card (settings rows) inputs and selects use `--fk-menu-bg` with a hairline instead, so they don't vanish into the card.
- **Choices inside a row:** a small segmented radio group (32 px track) on the right of a ruled line, as on Tone, where each app's line also shows a sample in the chosen tone.
- **Toggle:** 36 × 20 track, 16 px knob, ink when on.
- **Key caps:** `Kbd`: white cap, 1 px strong border with a 2 px bottom edge, Mukta 600. `size="lg"` inside headlines.
- **Dialogs:** `--fk-menu-bg`, radius 20, `--fk-elev-3`, 24 px padding, Martel 22 title, buttons bottom-right (secondary then primary).
- **Toasts (in window):** `--fk-inverse-bg`, radius 10, bottom-right, 16 px from the edges.

## 7. Motion

Motion is the movement of a pen: it settles, it does not bounce.

| Token            | Value                            | Use                                                    |
| ---------------- | -------------------------------- | ------------------------------------------------------ |
| `--fk-ease`      | `cubic-bezier(0.22, 1, 0.36, 1)` | Everything that appears or settles                     |
| `--fk-ease-move` | `cubic-bezier(0.65, 0, 0.35, 1)` | Layout moves (sidebar width)                           |
| `--fk-t-quick`   | 120 ms                           | Hover, press, menu exit                                |
| `--fk-t-base`    | 180 ms                           | Menus, toggles, tab switch, active ink line            |
| `--fk-t-size`    | 240 ms                           | Size changes (pill, expanding rows)                    |
| `--fk-t-layout`  | 300 ms                           | Sidebar ↔ rail                                        |
| `--fk-t-ink`     | 600 ms                           | One-off "ink" moments: progress fills, the done stroke |

- Menus enter with opacity and scale 0.97 → 1 from their anchor corner (`fk-menu`), exit with opacity only.
- The active nav ink line draws downward (`scaleY` 0 → 1) when the page changes.
- No springs, no overshoot, no confetti-style bursts in new work. Entrances never move more than 6 px.
- `prefers-reduced-motion` turns all of this off (handled in `flow.css`).

## 8. The pill

The pill is a small ink tile at the **bottom centre** of the screen that has the text cursor, 10 pt above the Dock (or the screen edge when the Dock is hidden). It is always dark ink, whatever the app theme, so it reads on any wallpaper.

Pill colours (CSS variables on the pill root, values from `BRAND`):

| Token          | Value                                | Use                      |
| -------------- | ------------------------------------ | ------------------------ |
| `--pill-night` | `#141a33`                            | Tile fill                |
| `--pill-rim`   | `rgba(141,156,255,0.28)`             | 1 px border              |
| `--pill-paper` | `#eef0ff`                            | Text, waveform           |
| `--pill-muted` | `#9aa3c7`                            | Secondary text, the rule |
| `--pill-ink`   | `#8d9cff`                            | Processing stroke, focus |
| `--pill-live`  | `#ff6b7d`                            | Live dot, errors         |
| shadow         | `0 8px 24px -6px rgba(8,10,24,0.45)` | All states except idle   |

States (sizes in pt; all size changes animate width and height over 240 ms with `--fk-ease`, never a scale):

| State                                                | Size / shape                                                                                                                | Contents                                                                                                                                                                                                                                                                                                                                                              |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Idle                                                 | 48 × 6 line, radius 3, fill `rgba(20,26,51,0.55)`, 1 px rim `rgba(238,240,255,0.5)`, no shadow. Invisible hit area 72 × 24. | Nothing.                                                                                                                                                                                                                                                                                                                                                              |
| Hover                                                | 184 × 34 tile, radius 10                                                                                                    | Left: mic button 26 × 26 (radius 8). Centre: "Hold fn to write" (Mukta 13/500, fn as a small key cap). Right, after a 1 px divider: meeting-notes button 26 × 26.                                                                                                                                                                                                     |
| Recording                                            | 152 × 36 tile, radius 10                                                                                                    | 6 px live dot at x 12 (opacity pulse 1 → 0.4, 1.2 s, ease in-out). A 1 px rule (`--pill-muted` at 50%) 11 pt from the top; 13 bars, 2 pt wide on a 5 pt pitch, **hanging down from the rule** (the shirorekha), length 2–16 pt from the mic level (attack 40 ms, release 140 ms). Right: elapsed `m:ss`, 12 px tabular, muted; on hover it becomes a × cancel button. |
| Processing                                           | 112 × 36 tile                                                                                                               | Bars retract up into the rule (180 ms). A 16 × 2 pt ink stroke runs along the rule left to right, 900 ms ease in-out, repeating.                                                                                                                                                                                                                                      |
| Done                                                 | back to idle                                                                                                                | The rule flashes ink once (180 ms) as the text lands, then the tile shrinks to the idle line.                                                                                                                                                                                                                                                                         |
| Error                                                | 36 tall, width fits text (max 320), radius 10                                                                               | 2 px live-red rule down the left edge, one short sentence in paper text. Leaves after 4 s or on click.                                                                                                                                                                                                                                                                |
| Toast                                                | 32 tall, radius 10, sits 8 pt above the pill                                                                                | Ink tile, paper text 13 px, at most one line. Rises 6 pt + fades in over 220 ms, stays 3.2 s, fades out over 120 ms.                                                                                                                                                                                                                                                  |
| Popup (meeting card, menu, word learned, permission) | 300 wide, radius 14, 8 pt above the pill                                                                                    | Follows the system theme: light `#ffffff` / dark `#1b202c`, `--fk-elev-2`, 14 px padding, Mukta. Enters with opacity + 6 pt rise over 220 ms, exits with opacity over 120 ms.                                                                                                                                                                                         |

The idle line can be dragged left or right along the bottom edge; it never moves vertically.

## 9. Other windows

- **Notepad (meeting notes window):** same tokens and fonts as the main window (copy the light/dark values from §2 into its own variables), same 52 px header, Martel title, ruled transcript lines, live state uses `--fk-live`, done state uses `--fk-ok`.
- **Onboarding:** one centred column, max 520 wide. Martel 34/42 title, one muted sentence, a large preview card (`Card tone="panel"`), primary button bottom-right. Progress is a row of short ink dashes, the current one ink and the rest `--fk-border-strong`.

## 10. App icon

macOS squircle filled with ink `#3245c4` (a very slight top-to-bottom darkening to `#2a3aa8` is allowed). On it, in paper white `#fbfcfd`: the Lipik mark, a horizontal headline stroke with four strokes hanging from it at different lengths (short, long, middle, shortest): the shirorekha of Devanagari and a voice waveform in one shape. Round caps, stroke weight about 1/10 of the icon. No letters, no microphone, no gradients beyond the slight darkening. The same mark in single colour (`LipikMark` in `src/flow/ui`) is used in the sidebar, the rail and the menu bar (template image, black on transparent).

## 11. Voice and copy

Lipik writes like a careful scribe: plain, short, calm. It says what happened and what to do next.

- Sentence case everywhere: titles, buttons, labels, menu items.
- Second person ("your words"), active voice, present tense.
- Short: titles up to 4 words, buttons 1–2 words with a verb first, descriptions one sentence.
- No exclamation marks, no emoji, no hype ("magic", "supercharge", "effortless", "10x"), no puns on speed or "flow".
- Numbers as digits. Keys as key caps, not in quotes.
- Hindi appears in the name and tagline only (लिपिक); the interface is English until a full translation exists.
- Tagline: **"Speak. Lipik writes it down."**

### Names of things

| Page id      | Nav label and page title | One-line description                              |
| ------------ | ------------------------ | ------------------------------------------------- |
| `dictation`  | Home                     | (greeting instead)                                |
| `notetaker`  | Meetings                 | Notes from your calls, written as you talk.       |
| `scratchpad` | Drafts                   | Think out loud. Keep what's worth keeping.        |
| `dictionary` | Vocabulary               | Names and terms Lipik should always spell right.  |
| `snippets`   | Snippets                 | Say a short cue, get the full text.               |
| `style`      | Tone                     | How your words are cleaned up in each app.        |
| `transforms` | Rewrite                  | Select text, then press a shortcut to rewrite it. |
| `insights`   | Stats                    | How much you write, and how fast.                 |

Settings sections by tab id, in nav order: `general` Dictation, `notetaker` Meetings, `vibe-coding` Code editors, `connectors` Connections, `mcp` AI tools; a hairline; `system` App, `privacy` Privacy, `account` Profile; About (`billing`) at the bottom. No group headings. There is no team tab. Paid-plan, billing, referral and "invite" surfaces are removed: Lipik is free and open source. About shows the version and the MIT licence.

The pill is "the pill" in help text. Recording is "dictation"; cleaned text is "cleaned up", never "polished" or "enhanced".

### Rewrite rules by category

| Category                          | Rule                                                                                                                                                                                   | Lipik examples                                                                                                |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Greeting (Home)                   | Time of day + first name; second line is the shortcut.                                                                                                                                 | "Good morning, Asha" / "Hold fn to write anywhere."                                                           |
| Page description                  | One sentence, what the page is for.                                                                                                                                                    | See table above.                                                                                              |
| Margin note (first-run explainer) | Title says the benefit in ≤ 6 words; body says how in one sentence; one action.                                                                                                        | "Spell names your way" / "Add a word once and every dictation uses it." / Add word                            |
| Buttons                           | Verb first, 1–2 words.                                                                                                                                                                 | Add word, New snippet, Import, Start, Save, Cancel, Delete, Try again                                         |
| Destructive confirm               | Name the thing and the result.                                                                                                                                                         | "Delete this snippet? This can't be undone." / Delete / Keep                                                  |
| Empty state                       | What will appear here, and how to make it appear.                                                                                                                                      | "No drafts yet. Start one with New draft."                                                                    |
| Errors                            | What happened, then what to do. No blame, no codes in the main line.                                                                                                                   | "Lipik can't hear your microphone. Allow it in System Settings › Privacy › Microphone."                       |
| Pill messages                     | Under 5 words.                                                                                                                                                                         | Hover: "Hold fn to write". Error: "Didn't catch that", "Microphone is off", "Couldn't paste. Copied instead." |
| Toasts                            | Past tense, under 6 words.                                                                                                                                                             | Copied, Word added, Saved to Drafts, Settings saved                                                           |
| Permissions                       | Why, then the one step.                                                                                                                                                                | "Lipik needs Accessibility to type for you. Open System Settings and turn on Lipik."                          |
| Onboarding steps                  | Plain imperatives.                                                                                                                                                                     | Let Lipik hear you · Let Lipik type for you · Choose your key · Try it once                                   |
| Stats                             | Plain nouns, numbers big.                                                                                                                                                              | Words written · Days in a row · Words per minute · Words cleaned up                                           |
| Labels, dates                     | Sentence case, relative where natural. Times follow the system's clock style (`toLocaleTimeString` with hour and minute) everywhere: Home, Meetings, Drafts, the pill and the notepad. | Today, Yesterday, Mon 21 Oct, 10:12 or 10:12 AM                                                               |
| Tooltips                          | The control's name, no full stop.                                                                                                                                                      | Copy, Play recording, Show sidebar, Appearance                                                                |
| Notifications                     | One line, past tense, links to the thing.                                                                                                                                              | "Meeting notes ready: Weekly sync"                                                                            |
| Tray / menu bar                   | Commands as verbs.                                                                                                                                                                     | Start dictation, Open Lipik, Check for updates, Quit Lipik                                                    |

## 12. Components (`src/flow/ui`)

| Component                             | Notes                                                                                                                |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `Button`                              | `primary` (ink), `secondary` (surface-2), `accent` (soft ink), `ghost`, `danger` (soft red). `md` 36 px, `sm` 30 px. |
| `IconButton`                          | 30 × 30, radius 8, muted icon, ink when `active`.                                                                    |
| `Toggle`                              | 36 × 20.                                                                                                             |
| `TextInput`                           | 38 px, optional leading icon.                                                                                        |
| `Card`                                | `tone="panel"` raised sheet, `tone="surface"` flat tint.                                                             |
| `SectionTabs`                         | Segmented control with optional actions on the right.                                                                |
| `ListGroup` / `ListRow` / `RowAction` | Ruled list.                                                                                                          |
| `HeroBanner` / `HeroButton`           | Margin note.                                                                                                         |
| `PageHeader`                          | Ruled page head.                                                                                                     |
| `Kbd`                                 | Key cap.                                                                                                             |
| `LipikMark`                           | Logo mark; `tile` draws the app-icon tile.                                                                           |

Before adding a new pattern to a page, check whether one of these covers it. A new shared pattern goes into `src/flow/ui` and into this document.
