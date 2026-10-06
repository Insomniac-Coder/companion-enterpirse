# Phase 1 map: the frontend

Part of the Phase 1 code map (the backend parts come from the mapping run and sit beside this file).
Written by reading `frontend/src` on 2026-10-06; about 10,000 lines of TypeScript and 2,000 of CSS.

## Where each screen goes

Today one React app has six top-level tabs (`App.tsx`: chat, models, resources, system, tools,
settings) and a right-hand panel. In the new design there are two apps that share one set of UI parts
(`frontend/src/ui/`, the style tokens, `MessageView`, `CodeBlock`): the **user app** (browser and
desktop) and the **admin dashboard**.

| Screen or part | Today | Goes to | Change |
|---|---|---|---|
| Chat tab: messages, composer, attachments, context gauge, session list and menu, share, fork, export | `MessageView`, `AttachChips`, `ContextBar`, `SessionMenu`, `ShareDialog`, `TimelinePanel` | User app | Only the user's own sessions; model picker lists allowed models with their abilities |
| Code mode: project launcher, project header, diff, git card, instructions card, file index card, knowledge card | `ProjectLauncher`, `CodeHeader`, `DiffModal`, `GitCard`, `InstructionsCard`, `RepoIndexCard`, `KnowledgeCard` | User app, **desktop app only** | Projects are folders on the user's PC; these read them through the local tool runner instead of the server |
| Agent progress, tool timeline, approvals, permission modes | `AgentChatProgress`, `ToolTimeline`, `PermissionsModal`, `permissionCopy` | User app | Approvals arrive over the run's event stream from whichever server holds the run; the permission mode menu offers only modes up to the admin's maximum, with the reason shown |
| Memory panel | `MemoryPanel` | User app | Memories are the user's own |
| Banners: model preparing, recovery, work status | `PrepareBanner`, `RecoveryBanner`, `WorkStatus` | User app | "Loading the model" becomes "the model is starting" for a pool scaled to zero; recovery covers the user's own runs |
| Models tab: library, load, delete, downloads, recommend, optimize | `ModelsPage`, `ModelLibraryItem`, `RecommendCard`, `OptimizeCard` | Admin dashboard | Becomes the model list: local and provider models, copies, which groups may use each |
| System tab: runtime status, start/stop, doctor, benchmark, start scripts | `RuntimePage`, `DoctorCard`, `BenchmarkCard`, `StartScripts` | Admin dashboard | Per model pool and per server instead of one model server |
| Resources tab and the machine panel (CPU, RAM, GPU meters) | `ResourcesPanel`, `ResourceChart`, `Rig` | Admin dashboard | Meters describe the servers, not the user's PC; one row per server |
| Calibration | `CalibrationCard`, `services/calibration.ts` | Admin dashboard | Run per llama.cpp model pool |
| Tools tab: tool registry, plugins | `ToolsPage`, `PluginsCard` | Both | Admin: the catalogue and group access. User: the allowed plugins with on/off switches, greyed out with the reason when the chosen model cannot use them |
| Settings panel | `SettingsPanel`, `settingsForm.ts`, `settingsModels.ts` | Both | Split by the plan's table; every setting shows locked or unlocked; the admin's version edits company and group values |
| Setup wizard | `SetupWizard` | Admin dashboard (one-machine install) | First-run setup of a server, not of a user |
| Command palette, welcome, toasts | `CommandPalette`, `Welcome`, `Toasts` | User app | Commands that need admin rights are hidden |

## What assumes the screens and the backend are on the same PC

| What | Where | Change |
|---|---|---|
| Every request goes to the page's own address (`API = ''`) | `services/api.ts:2`, the `req` helper | The server address comes from the desktop app's saved servers (or the page's origin in a browser); every request carries the sign-in token |
| Folder picker runs on the server (`POST /api/system/pick-folder` opens a native dialog where the backend runs) | `services/api.ts:473`, backend `pick_folder` | The desktop app opens its own folder dialog; the browser version has no project folders |
| The current project is kept by id in the browser's storage (`companion.workspace`, `companion.projectGroups`) | `App.tsx` lines 121-125, 358, 619, 696 | Projects belong to the user and to one PC; the app keeps which project is open on this PC |
| Permission mode remembered in the browser (`companion.permissionMode`) | `App.tsx` 175, 447 | Saved as the user's setting on the server, capped by the admin's maximum |
| Streams are read with `fetch` and a body reader | `services/api.ts` 388, 658, 784; `services/calibration.ts` 186 | Good news: a `fetch` stream can carry the sign-in token in a header (the browser's `EventSource` could not), so streaming needs no redesign, only the token and a resume point after a reconnect |
| One loaded model drives the top bar, banners and the "start the model" actions | `App.tsx` (`requestLoad`, `guardedSwitch`), `RuntimePage` | The user picks a model per conversation from the allowed list; loading is the pool manager's job |

## Kept as they are

Theme, density, reduce motion, panel width and pinned sessions stay in the browser's storage: they
are per-device conveniences. The frontend's pure helpers and their tests (`services/*.ts`,
`*.test.mjs`) move with the user app unchanged unless a row above names them.

## Size

Splitting into two apps and a shared package, adding the server address and sign-in to `req`, and
moving the folder picker are each a few days. Rebuilding the models, system and resources tabs as the
admin dashboard's model and server screens is the larger part, and depends on Phase 2's model list.
