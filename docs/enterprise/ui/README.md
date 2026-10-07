# The user app's look: Graphite

Chosen by the owner on 2026-10-06 from three directions (`directions/`: A Clearing, B Atrium,
C Graphite): **C, Graphite, with a light variant.** It is built in Phase 1 task 9, when the screens
split into the user app and the admin dashboard; the admin dashboard uses the same look.

The mockups show the target screens, not today's: no CPU/RAM/GPU meters and no Load button (they go to
the admin dashboard in Phase 1, and model loading becomes the server's job in Phase 2). A person picks a
model per conversation from the approved list. People choose Dark, Light or Match system in settings, as
today.

| Picture | What it shows |
|---|---|
| `directions/C-Graphite.png` | Dark |
| `directions/C-Graphite-light.png` | Light |
| `directions/source/` | The mockups' HTML and CSS (`render.sh` prints them at 1440x900 with headless Edge) |

## What makes it Graphite

- **A command bar** at the top of every screen: search, or run a command (Ctrl K).
- **One quiet line above every answer**: who answered, which model, how many documents and tools it
  used, whether it stayed inside the company, how long it took. In code: a small monospace line.
- **Inside the company** is always visible: in the top bar, in each answer's line, and in the model
  picker. Blue (the signal colour) while data stays inside; amber when it leaves (Phase 2, outside
  providers).
- **Actions that need approval** (send an email, change a ticket) arrive as an amber "Waiting for you"
  card with keyboard keys: Enter sends, E edits, Esc cancels.
- **Sources** sit under the answer as small numbered chips that match the numbers in the text (RAG).
- **A dense sidebar**: New chat and Search with their keys, the workspace (Chats, Code tasks, Documents,
  Tools, with counts), recent conversations with a dot that turns amber when one is waiting for you,
  and the person at the bottom.

## Colours

Exactly as in the mockups' `:root`. One signal colour (blue) and one "needs you" colour (amber); red only
for deleting and errors.

| Role | Dark | Light |
|---|---|---|
| Page | `#0e0f12` | `#fbfbfc` |
| Sidebar | `#121317` | `#f4f4f6` |
| Panel (cards, inputs) | `#16181c` | `#ffffff` |
| Selected row | `#1c1e23` | `#e9e9ee` |
| Lines | `#24262c` / `#30333a` | `#e6e6eb` / `#d7d8de` |
| Text, strongest to faintest | `#ecedf0` `#b4b8c1` `#7d828d` `#555a64` | `#18191d` `#474a52` `#7a7e88` `#a4a7af` |
| Signal (inside, links, citations) | `#9db9ff` | `#3f63d8` |
| Waiting for you | `#e8b05c` | `#a8650a` |
| Main button | text colour on page colour (inverted) | the same |

## Type

Inter for everything people read (13.5–14 px, slightly tightened); JetBrains Mono for the machine:
the line above each answer, keys, table headings, counts, code. Both are bundled with the app (no
network).

## Built (Phase 1 task 9, 2026-10-07)

Both apps use it: the colours are the tokens at the top of `frontend/src/workbench.css` (the old
accent names stay, so `--tally` is now the blue signal), the fonts are in `frontend/src/fonts` with
their licence. Built: the dense sidebar (Workspace with counts, Recent with status dots, the person
with Settings at the bottom), the top bar's crumb, command bar and boundary mark ("Inside the
company" on a server, "On this PC" on a laptop), the quiet line above every message, messages as
plain text divided by rules, the composer's model chip, the main button in inverted ink, the amber
"Waiting for you" approval card. Still to come with the features they belong to: source chips (RAG,
Phase 6), a model picker per conversation (the model list, Phase 2), approval keys Enter/E/Esc
(the card has buttons until approvals reach the chat with the desktop app, Phase 3), the amber
"outside" state (outside providers, Phase 2).
