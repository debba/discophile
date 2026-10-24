# Discophile website design guide

This guide describes the visual language of [discophile.app](https://discophile.app) and how to carry it into related Discophile interfaces. It covers brand, typography, colors, layout, controls, motion, and accessibility.

The implementation references are [the global stylesheet](app/globals.css), [RootShell](components/RootShell.tsx), and the components linked below. The stylesheet is the source of truth for existing values; update this guide when changing shared design decisions.

## Visual direction

Discophile feels like a personal record collection: warm, tactile, expressive, and easy to browse. Rose backgrounds, plum text, condensed display typography, and a restrained raspberry accent give the site its identity.

Use one expressive element to lead a composition, such as the record in the home-page hero. Balance it with readable text, generous spacing, and clear actions. Related application screens should retain that identity while giving account information, forms, and administrative tasks a calmer setting.

The recognizable elements are:

- The Discophile record-player icon and wordmark.
- Bricolage Grotesque headings and Public Sans body text.
- Rose, paper, plum, and raspberry surfaces and accents.
- Rounded buttons, soft corners, and restrained shadows.
- Record grooves, sleeves, labels, and circular details used as supporting motifs.
- Real product imagery or clearly identified demonstration content.

## Brand and assets

The header pairs a 36-pixel app icon with the Discophile wordmark. Use the existing assets rather than redrawing the logo or substituting a generic music symbol.

- [Header icon](public/assets/icon-72.png).
- [SVG favicon](public/assets/favicon.svg).
- [Apple touch icon](public/assets/apple-touch-icon.png).
- [Header implementation](components/SiteHeader.tsx).

Keep the icon's proportions and leave space between it, the wordmark, and neighboring navigation. A related service may add its name as a secondary label while keeping Discophile as the primary brand.

Feature icons use Phosphor's regular weight through [Icon](components/Icon.tsx), matching the desktop app's icon family. Use consistent optical size and weight. Decorative icons are hidden from assistive technology; actionable icons have a visible label or accessible name.

## Typography

Fonts are loaded with `next/font/google` in [RootShell](components/RootShell.tsx), exposed as `--font-display` and `--font-body`, and consumed through `--display` and `--body`.

| Role | Current website treatment |
| --- | --- |
| Display and headings | Bricolage Grotesque, width axis usually `80` |
| Body and controls | Public Sans, regular, medium, and semibold |
| Body copy | `1.0625rem`, line height `1.6` |
| Home-page H1 | `clamp(2.9rem, 7.4vw, 6.4rem)`, weight `750` |
| H2 | `clamp(2.2rem, 4.6vw, 3.9rem)`, weight `720` |
| H3 | `1.35rem`, weight `650` |
| General headings | Line height `0.98`, letter spacing `-0.02em` |
| Inner-page H1 | `clamp(2.8rem, 6.4vw, 5rem)` |

These sizes describe the marketing site. In account or administrative interfaces, retain the font pairing and heading character while reducing display sizes to suit denser information. Multi-line labels, tables, forms, and operational messages need comfortable reading line heights.

Keep prose columns readable: the default paragraph measure is `34em`; some inner-page introductions allow `38em`. Use balanced headings and natural wrapping. Reserve uppercase treatment for short labels and decorative copy, rather than paragraphs or form labels.

## Color tokens

Use semantic CSS variables so the same component can follow light, dark, and automatic appearance. These are the current base values and dark-mode overrides.

| Token | Light | Dark | Purpose |
| --- | --- | --- | --- |
| `--rose` | `#f4d9e2` | `#24111f` | Page background |
| `--rose-deep` | `#ebc3d1` | `#2f1729` | Alternate sections and soft emphasis |
| `--paper` | `#fff6f9` | `#331a2e` | Reading surfaces and dialogs |
| `--surface` | `#fff` | `#3d2137` | Raised surfaces and inputs |
| `--tint` | `#fde7ee` | `#4a2240` | Selected options and status surfaces |
| `--ink` | `#2b0d22` | `#fbeef4` | Primary text and outlines |
| `--ink-soft` | `#6a3f58` | `#d9bfce` | Secondary text and field borders |
| `--raspberry` | `#c72d62` | Inherits light value | Brand accent and primary actions |
| `--plum` | `#661d49` | Inherits light value | Deeper brand accent and primary hover |
| `--link` | `#a3204f` | `#ff9dbb` | Text links |
| `--accent-text` | `var(--plum)` | `#ffb3cb` | Emphasized text |
| `--vinyl` | `#1d1020` | `#130811` | Record and dark decorative surfaces |
| `--line-rgb` | `102 29 73` | `251 238 244` | Borders with an explicit opacity |
| `--shadow-rgb` | `61 12 40` | `0 0 0` | Shadows with an explicit opacity |

Use `--link` for normal text links on rose. The base raspberry accent is too light for normal-size text on that background. Keep surface and foreground pairings intact when adding new components, and check their contrast in both themes.

Color must not be the only indication of selection, an error, a plan, or a status. Pair it with text, shape, an icon, or another explicit cue. Treat success, warning, and destructive states as semantic additions that require their own contrast checks; the current brand palette is not a complete status palette.

## Appearance behavior

The website supports Automatic, Light, and Dark. An explicit choice sets `data-theme="light"` or `data-theme="dark"` on the root element; Automatic follows `prefers-color-scheme`.

[ThemeSwitcher](components/ThemeSwitcher.tsx) stores an explicit choice under `discophile-theme` in local storage. The small initialization script applies it before first paint to avoid a flash of the other theme. The browser theme color follows the effective appearance.

Related services should keep these three choices and equivalent behavior. Local storage is scoped to the origin: sharing the visual identity does not automatically share appearance preferences between the website and a subdomain.

## Layout and spacing

The website uses a centered content width of `1240px` and a responsive gutter of `clamp(16px, 5vw, 72px)`. Large marketing sections use fluid spacing, commonly `clamp(60px, 9vw, 120px)` vertically.

The home-page hero and feature rows use asymmetric two-column layouts, becoming a single column at `860px`. Header navigation switches to its mobile menu at `720px`.

Use those values as references rather than forcing every product screen into the home-page composition. For an account or administrative interface:

- Keep a recognizable header with concise navigation and appearance/language controls.
- Use smaller, consistent spacing between related form fields and data rows.
- Group information by task, with one clear primary action per form or section.
- Allow names, email addresses, translated labels, and server URLs to wrap safely.
- Stack columns when they no longer fit. Use contained horizontal scrolling for tables only when it is needed.
- Keep decorative artwork out of dense tables and forms.

## Surfaces, borders, and depth

Paper and raised surfaces separate reading and interaction areas from the rose page. Border colors derive from `--line-rgb` or the appropriate text token, with restrained opacity.

Existing examples include 20-pixel summary surfaces, a 24-pixel TestFlight dialog, 12-pixel input corners, 16-pixel code blocks, and pill-shaped buttons and badges. Choose a small set appropriate to the interface rather than adding a different radius to each element.

Shadows establish elevation for dialogs, menus, and product imagery. Routine data rows and account sections should rely primarily on spacing, surfaces, and subtle dividers. Record grooves and display gradients belong to the illustrative layer.

## Buttons and links

The current `.button` treatment uses:

- A minimum height of `48px` and horizontal padding of `22px`.
- A pill radius of `999px` and a `2px` border.
- Public Sans at `1rem`, weight `600`, line height `1.2`.
- A `10px` gap between text and supporting icons.
- Short background and text-color transitions, around `0.15s`.

A primary button uses a raspberry background and border with white text; hover uses plum. A secondary button starts transparent with an ink outline and text, then fills with ink on hover. Plain links use the link token and visible underline behavior.

Use a button for an action and an anchor for navigation. Give loading, disabled, success, and error states explicit behavior and accessible feedback. During a pending submission, prevent duplicate writes and keep the control's purpose understandable.

Compact controls may reduce visual padding while preserving a comfortable touch target. The site's mobile navigation and dialog close controls use at least 44-pixel targets.

## Forms and dialogs

[TestFlight](components/TestFlight.tsx) and its styles provide the existing reference for forms and modal interaction:

- Visible labels, with supporting hints when a choice needs explanation.
- Inputs at least `48px` high, with `12px` corners, a `2px` border, and a raised surface.
- Input text at `1.0625rem`; textareas can resize vertically.
- Invalid fields marked with `aria-invalid`, an explicit message, and a stronger border.
- Status messages placed near the form, with text explaining the outcome.
- Focus moved appropriately when opening a dialog, finding an invalid field, or showing completion, and restored to the trigger when closing.
- A clear close action and Escape behavior, with focused fields kept clear of sticky dialog headers.

Carry these interaction rules into authentication and account forms. Social sign-in buttons should look native to Discophile while keeping provider identity recognizable. Administrative changes should explain their target, effect, and result. Destructive operations need a distinct, explicit action.

## Data views

Existing website tables use left-aligned headers, generous cell padding, horizontal separators, and minimal surrounding decoration. See the comparison and policy styles in [globals.css](app/globals.css) and [PolicyTable](components/PolicyTable.tsx).

For related dashboards, preserve that clarity. Use Public Sans for row content, explicit text for plan and access states, and tabular numerals where aligned quantities benefit. Search, pagination, empty results, errors, and actions should remain usable on narrow screens.

Do not communicate a future capability as available merely because its plan name is displayed. The visual treatment must distinguish current state, planned benefits, and actions the person can perform now.

## Motion and imagery

The website's record, tonearm, device compositions, and video create a sense of listening. [SpinningRecord](components/SpinningRecord.tsx), [Shots](components/Shots.tsx), [DesktopShot](components/DesktopShot.tsx), and [HeroVideo](components/HeroVideo.tsx) are the implementation references.

Product interfaces should use short, purposeful transitions for state changes and reserve expressive animation for suitable introductory or celebratory moments. Avoid decorative looping motion around forms and administrative work.

Respect `prefers-reduced-motion`, avoid layout shifts during loading, and keep essential information available without animation or hover. Use real screenshots where appropriate; fictional content must be clearly identified as demonstration content.

## Language and accessibility

English is the source language, and Italian is included. Website interface strings live in [lib/i18n.ts](lib/i18n.ts). Reuse established product vocabulary where it applies, and allow for the different lengths of translated labels.

Use plain, concrete labels for actions. Explain what changes and where, especially for account access, connections, and destructive operations. Keep implementation details out of ordinary user flows unless they help the person decide.

New interfaces must preserve:

- Semantic headings, landmarks, labels, and a skip link.
- Visible keyboard focus and a logical reading/tab order.
- Text and control contrast in light, dark, and forced-color modes.
- Meaningful status announcements and errors associated with the affected input.
- Non-overlapping touch targets and layouts that tolerate zoom and longer text.
- Dialog and menu keyboard behavior, including focus restoration.
- Usability when reduced motion is enabled or a pointer is unavailable.

## Applying the identity to related products

Reuse the fonts, semantic color roles, logo, control character, theme choices, and writing style. Adapt information density and page composition to the task. A sign-in form, account dashboard, or administrative table should be recognizably Discophile without needing the marketing site's full hero or promotional effects.

Keep this guide as the shared reference rather than maintaining disconnected copies of the palette and component rules. Application-specific behavior belongs in that application's architecture and interaction documentation.

## Review checklist

Before delivering visual changes, inspect representative screens in both appearances and languages, at desktop and narrow phone widths. Check keyboard navigation, focus, zoom, loading, validation, empty states, long values, and reduced motion. Verify that the implementation still matches these design decisions and update the relevant source or documentation when it does not.
