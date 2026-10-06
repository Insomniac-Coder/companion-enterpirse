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

Tests: backend 536 on Windows and 534 on Linux (Ubuntu under WSL), frontend 114, against a throwaway private database
that ends with the run; the test database skips crash-safe writes, which took a third off the run.

## Not in Phase 1

Several models at once and outside providers (Phase 2); the desktop app and the device channel, so the
agent's file and command tools run on the user's PC (Phase 3); plugins and MCP (4); Jira and Outlook
(5); RAG (6); the rest of the guards and the audit viewer (7); several servers, Redis and scaling (8).
