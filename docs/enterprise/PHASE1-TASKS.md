# Phase 1: Foundations, task list

Built from the Phase 1 code map (`phase1-map/`). Phase 1 ends when **two colleagues share one server
and each sees only their own work, and the admin locks a setting and the user sees it locked.** It
stays on one server; running across several servers is Phase 8, and the design keeps that open.

Sizes use the map's scale: S is under a day, M a few days, L a week or more. Weeks are one
developer's working weeks.

| # | Task | What it does | Main code today | Size |
|---|---|---|---|---|
| 1 | **PostgreSQL** | Replaces the single database file and the one lock every request waits on (93 places) with a PostgreSQL connection pool; schema changes become numbered migration files; the SQLite-only parts are rewritten (ordering by row number, `INSERT OR REPLACE`, `LIMIT -1`, pragmas, the check-then-add-column migrations) | `storage.rs` (18 tables), `AppState.storage` | L, ~3 weeks |
| 2 | **Import the Companion's history** | A one-off copy of an existing Companion database into PostgreSQL, owned by the person who imports it | new | S, ~3 days |
| 3 | **Company sign-in** | Microsoft Entra ID sign-in (OpenID Connect) for people, API keys for software; every route checks who is calling; the localhost-only guard becomes a list of allowed addresses | `router`, `trusted_loopback_origins` | L, ~2 weeks |
| 4 | **Users, groups and roles** | Users and groups (groups come from the company directory); the roles user, team admin, platform admin and auditor; admin-only routes (models, downloads, calibration, company settings, system checks) need an admin role | new tables; route groups in `api.rs` | M, ~1.5 weeks |
| 5 | **An owner on every record** | The ten tables of personal data get an owner (directly or through their conversation); every handler that fetches by id checks it (about 60); sharing and forking between people become explicit | `storage.rs`, conversation, message, attachment, artifact, memory, session handlers | L, ~2 weeks |
| 6 | **Settings in three levels, with locks** | Company default, then group, then the person's own choice; any setting can be locked; the 20 places that read the one settings object read "this person's settings" instead; the screens show a locked setting with the reason and the server enforces it; admin settings that are saved today but never applied are built: allowed and blocked folders, trusted web addresses, the attachment size limit, the command time limit, parallel sessions, log masking | `settings.rs`, `get_settings` / `put_settings`, `SettingsPanel` | L, ~2.5 weeks |
| 7 | **Permissions and replies per person** | The permission mode and "Allow for session" grants belong to each person, capped by the admin's maximum (today one global setting: switching to Auto approves everyone's waiting actions); one reply per conversation instead of one for the whole app; Stop stops only your own reply | `PermissionManager`, `put_permission_mode`, `resume_auto_approved_runs`, `GenerationTracker` | M, ~1 week |
| 8 | **Audit records from day one** | One record per request, tool call, approval, admin change and sign-in: who, when, from which device, what, which model, tokens, and how it was allowed (tool calls already say how since 2026-10-06). Conversation export is already gone (2026-10-06): people save the program's own logs as a zip instead. The tamper-evident chain and the viewer follow in Phase 7 | `tool_executions`, `model_requests`, `search_runs` | M, ~1 week |
| 9 | **Two apps from one set of screens** | `apps/client` (the user app), `apps/admin` (the dashboard), `packages/ui` (shared parts); every request carries the server address and the sign-in token; dashboard v1: users and groups, company and group settings with locks, the tool record; the models, system and resources tabs move to the dashboard as they are (rebuilt in Phase 2); both apps in the chosen look, Graphite with dark and light (`ui/README.md`) | `frontend/src` (see `phase1-map/frontend.md`) | L, ~2.5 weeks |
| 10 | **Checks that people stay apart** | Tests that one person cannot see or change another's conversations, attachments, memories, runs or records; the architecture notes and handoff updated | tests | M, ~4 days |
| 11 | **Overwriting needs a read first** | A file the run has not read cannot be overwritten in one go (Claude Code's rule): "change one line" sent as a whole-file write wipes everything else in the file, the same failure as the delete bug fixed on 2026-10-06 | `run_loop`, `write_file` | S, ~1 day |

**Order.** 1 first (everything else stores through it), then 2, 3 and 4; 5 to 8 after them; 9 can
start alongside 5 once 3 is done; 10 grows with each task. 11 can go in at any time.

**Estimate.** About 17 weeks of one developer's work by the sizes above. The map's per-entry sizes add
up to more (about 225 working days), but its entries overlap: the database move alone is counted once
as a whole and again table by table. I will re-estimate after tasks 1 and 2 against how long they
actually took.

## Owner decisions (2026-10-06)

1. **The one-machine install (a laptop) runs as one local person**, reachable only from the same PC,
   as the Companion does today. A company account is not required yet: the project is in development
   and not yet approved for adoption. Sign-in becomes required when a server accepts connections from
   other machines.
2. **PostgreSQL everywhere**, the laptop included, installed with the app: one storage layer to build
   and test. The app manages its own private PostgreSQL; the install script is part of the repo
   (`scripts/get-postgres.py`).
3. **Windows and Linux both** ("make the whole project OS agnostic"): every script and the backend run
   on either, checked on both.

## Progress

| # | Task | State | What was built |
|---|---|---|---|
| 1 | PostgreSQL | Done 2026-10-06 | sqlx pool; `backend/migrations/0001_initial.sql` (18 tables); every storage call async; a private PostgreSQL started and stopped by the app (`pgsql.rs`) from `runtime/pgsql/`, installed by `scripts/get-postgres.py` from the pinned, checksum-checked `runtime/postgresql.lock.json` (Windows: the EnterpriseDB build, 141 MB kept; Linux: built from the official source, 27 MB); `companion-backend backup` (pg_dump) replaces the SQLite backup script. Three places where the old lock had made several steps one were made safe again: a run's final event and its reply text are saved together; editing a conversation saves only the fields it changed; re-indexing a file replaces its chunks as one step |
| 2 | Import the Companion's history | Done 2026-10-06 | `companion-backend import-sqlite <file>`: every table, in the file's order, column by column (an older file's missing columns take their defaults), one transaction, safe to run twice. Owners arrive with task 5 |
| 3 | Company sign-in | Done 2026-10-06 | `auth.rs` + `migrations/0002_sign_in.sql`: OpenID Connect sign-in (authorization code with PKCE; the ID token's signature, issuer, audience, expiry and nonce checked by the `openidconnect` crate) for Entra ID or any standard provider; session cookie; API keys; every `/api/` route knows who is calling; without sign-in the server refuses any address but this PC's, and is the local person. Screens: a sign-in page, the person in the sidebar, Settings > Account with API keys (only on a server with sign-in). Checked against a stand-in provider in the tests (tampered tokens, replayed and stale links, expired sessions, withdrawn keys) and live in a browser. Not done here: groups and roles (task 4), owners on records (task 5), the sign-in audit record (task 8), the desktop app's sign-in (Phase 3) |
| 4 | Users, groups and roles | Done 2026-10-06 | `roles.rs` + `migrations/0003_roles.sql`: groups from the ID token's `groups` claim (renewed at each sign-in; kept as they were when the token leaves them out); roles platform admin, team admin (per group) and auditor from the directory's `roles` claim, `COMPANION_PLATFORM_ADMINS`, or grants made here; 25 shared routes need a platform admin; `/api/admin/users`, `/api/admin/groups`, role grants. Screens: someone who is not a platform admin sees no model, download, diagnostics or plugin controls, read-only settings and permission mode. **Interim until tasks 6 and 7:** the one settings object and the one permission mode apply to everyone, so only a platform admin changes them (plan approval switches the mode, so it is an admin's too). Found on the way: the message list read messages and their journals in two queries, so a reply finishing in between showed its new journal beside its old text (the old lock hid it); now one snapshot, with a race test that fails without it |
| 5 | An owner on every record | Done 2026-10-07 | `ownership.rs` + `migrations/0004_owners.sql`: `user_id` on conversations, projects and memories (existing records: the local person); one check before every handler for paths that name a record, 404 for anyone else's (admins included); body ids checked; lists scoped; runs carry their person. Found on the way: saved memories marked "global" went into **everyone's** prompts; an agent could be started in any folder, someone else's project included (now only inside the caller's own saved projects); a direct tool call could file its record in someone else's conversation; the folder chooser opened on the server's own screen (refused on a server; the project dialog takes a typed server folder). `import-sqlite --owner <email>`. A two-person test (Ada and Bob) covers lists, 21 paths, body ids and the prompt's memory; it fails with either protection taken out |
| 6 | Settings in three levels, with locks | Done 2026-10-07 | `settings_levels.rs` + `migrations/0005_settings_levels.sql`: every field personal, policy or machine; company, then groups (by priority), then the person; company and group locks; a person's reads use their settings; the settings screen saves their own choices and shows locked fields with the reason; API for group values, locks and company defaults (dashboard: task 9). Built and enforced: allowed/blocked folders (also for projects saved earlier), the network rule (`ask`/`selected`/`disabled`), attachment limits, the command time limit, log masking. Settings got a `version`: defaults that were saved but never applied (network "disabled", a 120 s command limit) are read once as today's behaviour (search on request, 30 minutes). Found on the way: attachments over about 1.5 MB never got past the server's built-in 2 MB request limit. Interim until task 7: the permission mode is still one for everyone. Parallel sessions: task 7 |
| 7 | Permissions and replies per person | Done 2026-10-07 | Each person has their own permission mode and their own grants and one-time approvals (`AppState::permissions_of`); the policy field `agent.max_permission_mode` (default auto) caps the mode, and a mode chosen before a lower cap reads as the cap; switching to Auto releases only that person's waiting actions; one reply per conversation (`GenerationTracker` keyed by conversation and person) instead of one for the whole app; Stop names the conversation and stops only the caller's reply. The task 4 interim (the mode was a platform admin's) is gone: the picker and Shift+Tab work for everyone, and modes above the cap are greyed out. Parallel sessions: not built in Phase 1. The old setting was a model-server knob (parallel slots), which the code map moves to the pool manager's per-pool settings in Phase 2 |
| 8 | Audit records from day one | Done 2026-10-07 | `audit.rs` + `migrations/0006_audit.sql`: one record per model request, tool call (refused ones too, also in agent runs), web search, approval (the person's own, or released by a mode change), admin or settings change (allowed or refused), sign-in (and refused sign-ins), sign-out and API key change: who and how they acted (browser, API key, their agent run), when, the connection's address and the browser's or app's name, what (tool arguments, query or request body, masked and cut to 1,000 characters; no message text), model, tokens, how it was allowed, outcome. The table only grows (a trigger refuses changes). The writers need a `Who`, so new code cannot leave one out; a new admin route is recorded without being listed. `GET /api/admin/audit` for platform admins and auditors. Found on the way: the log masking did not know the search key's setting name (`brave_key`); a Windows checkout gave the first migration CRLF line endings and Linux LF, so a database (or a backup) made by one build would not open with the other (sqlx checks each migration's bytes): migrations are LF everywhere now (`.gitattributes`, and a test), and `build.rs` rebuilds when a migration changes. Phase 7: fingerprint chain, retention, viewer, export |
| 9 | Two apps from one set of screens | Done 2026-10-07 | One Vite build, two pages: Companion (`/`, `src/App.tsx`) and the dashboard (`/admin/`, `src/admin/`), sharing `src/ui`, `src/services` and the settings screen; every request through `services/server.ts` (server address and token, for the desktop app). Dashboard v1: people and groups with roles, company settings, each group's values and priority, locks, the audit records (the tool record included), and the models, runtime and resources screens moved as they were (tools and plugins stayed in Companion, as people's own: running a plugin's tool is now open to everyone, in their own project and under their own permission mode, where it used to be an admin's, in the server's temporary folder when no project was named); the user app no longer loads models. Both in Graphite, dark and light (`docs/enterprise/ui/README.md` lists what is built and what waits for its feature). A single repo package rather than `apps/` + `packages/`: two pages of one build share everything without a workspace tool; the desktop app (Phase 3) wraps the user page. After the owner's review: chat bubbles (the person's on the right, the assistant's on the left); Companion never links to the dashboard; Settings in Companion is a dialog from a settings button, with each person's own settings only (General, Chats, Memory and context, Code and agents, Tools and plugins, Keyboard, Account); the dashboard has Model server, Web search, Rules, Privacy and logging, and People's defaults with a lock beside each field; sampling and the startup model became the company's; an admin's own Settings no longer writes company values (a company-locked personal field used to change for everyone when an admin edited it). Next, per the owner: per-model settings on the Models page, servers in place of the resources page, and people choosing among the models the admin hosts. Found on the way: startup that refused its database left the private PostgreSQL it had started running |

Tests: backend 577 on Windows and 575 on Linux (Ubuntu under WSL), frontend 130 on both, against a throwaway
private database that ends with the run; the test database skips crash-safe writes, which took a third off the run.
Linux also checked end to end (2026-10-06): `run.sh` from a fresh copy (model runtime built CPU-only, private
database, UI), `scripts/e2e/e2e.mjs` with a real model, and Ctrl+C leaving nothing running; the same script on
Windows with the same model failed in the same places (the model's own mistakes), so none was the OS.

## Held for later (owner, 2026-10-06)

- **Projects on a shared server.** A project is a folder on the machine the agent runs on; until the
  desktop app runs the tools on each person's own PC (Phase 3), a signed-in person can register any
  folder of the server as a project. Task 6 builds the admin's allowed and blocked folders, which
  closes this for Phase 1; until then a server with sign-in is for trusted colleagues only.

- **A live Microsoft Entra ID sign-in test.** Development happens on a personal laptop; the project
  moves to the work laptop, where IT can make the app registration (README, "Sign-in"). Until then
  sign-in is tested against stand-in identity providers only (the backend tests' own, and a
  throwaway one for browser checks). Do it on the work laptop before anyone else uses a server.
- **An independent security review of the sign-in code** (proposed: three reviewers plus a checker
  per finding, up to nine agents). Held: too many agents for now. The sign-in code was reviewed by
  its author only (one hole found and fixed: a return link now works only in the browser that
  started the sign-in).

## Not in Phase 1

Several models at once and outside providers (Phase 2); the desktop app and the device channel, so the
agent's file and command tools run on the user's PC (Phase 3); plugins and MCP (4); Jira and Outlook
(5); RAG (6); the rest of the guards and the audit viewer (7); several servers, Redis and scaling (8).
