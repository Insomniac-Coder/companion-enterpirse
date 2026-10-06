# Phase 1 code map

Where every part of today's code goes in the enterprise design, and what it assumes today that breaks
with many users, many models, many servers, or tools running on the user's PC. Made on 2026-10-06 by
four read-only agents (owner-approved, at most two at a time) plus a frontend pass by hand; checked
against the code afterwards (see "Checked by hand").

| File | Contents |
|---|---|
| `job1.json` | The agent loop and its tools: 98 entries, 19 surprises |
| `job2.json` | Running models: 89 entries, 16 surprises |
| `job3.json` | Users, conversations and settings: 117 entries, 16 surprises |
| `job4.json` | Second opinion on job 1: 40 entries confirmed, 14 challenges, 6 missed entries |
| `frontend.md` | Which screen goes to the user app, the admin dashboard or both |

Each entry has: what the code does, what it assumes today, where it goes, the change, a size (S under a
day, M a few days, L a week or more), the evidence and a confidence. The sizes are the agents' guesses
per entry; entries overlap between jobs, so they are not added up here. The Phase 1 task list and its
estimate are built from this map as the next step.

## Where the backend goes

| Destination | Entries |
|---|---|
| API servers | 91 |
| Model pool manager | 40 |
| Desktop app (the local tool runner on the user's PC) | 38 |
| Workers | 36 |
| Shared library (pure code used in more than one place) | 51 |
| Admin-only endpoints and settings | 31 |
| Remove | 17 |

## The large pieces (size L)

- **The agent loop** (`run_loop`, one 1,540-line function, all its state in local variables), the
  live-run record (`LiveRun`: events, approvals, stop), waiting for approval, and running a tool. They
  move to the workers; run state, events and approvals become database records, so any API server can
  show a run that a worker is carrying out.
- **Starting an agent run** (`spawn_agent_run`) and **the chat path** (`chat_sse`, 900 lines): sign-in,
  policy checks, model choice per request, document search, the outgoing-data scan and the audit record
  all plug in here.
- **Chat's own use of project tools** (`run_chat_tool_round`): chat in a code session reads the user's
  project too, so it needs the device channel as well, not only the agent.
- **The model server code** (`LlamaServerManager`, `RunningSidecar`, `start_sidecar`): "at most one
  model running" becomes a registry of model copies run by the pool manager; downloads become admin
  jobs.
- **Server state, the router and settings** (`AppState`, the localhost-only guard, `put_settings`,
  `AppSettings`): split by role; sign-in and role checks on every route; settings become company /
  group / user values with locks.
- **The database layer** (`Storage`, its schema, the knowledge table): one SQLite connection behind one
  lock becomes a PostgreSQL pool, with an owner on every record.

## What the map adds to the design

These follow from decisions already made; none needs a new decision, but the plan should say them.

1. **Anything that touches the user's PC goes through the desktop app, chat included.** The draft only
   named the agent; chat in a code session also lists, reads and searches the project, and reads its
   instructions file.
2. **The desktop app ships its own copy of the screens and shows approvals in its own window.** If the
   app displayed screens sent by the server, a compromised server could send a screen that clicks
   "Allow" by itself or adds an allowed folder. The installer carries the screens; the server sends
   data only.
3. **The device channel needs more than a request and a reply:** an id per request, progress messages
   ("waiting for approval", "running"), and results kept on the PC and delivered after a reconnect.
   Some steps are not safe to repeat (appending text, running a command), so a lost reply must never
   re-run them.
4. **A whole agent run stays on one model copy**, not only a conversation: the run's completion checks
   and memory notes reuse what that copy has already read.
5. **llama.cpp copies serve one conversation at a time today** (`--parallel 1`). Serving many people
   from llama.cpp means more copies, or several slots per copy with the context split between them;
   vLLM handles many at once. This feeds GPU sizing.
6. **Every request names its model.** Today chat and agent use "whatever model is running".
7. **One chat reply at a time for the whole app** becomes one per conversation.
8. **Admin settings must be built, not only moved.** Allowed and blocked folders, trusted hosts, log
   redaction, the attachment size limit, the command timeout and parallel sessions are saved today but
   nothing reads them.
9. **The model's tool check runs inside the model load and holds it up**; it becomes a separate job.

## Gaps that must not be carried over

Found by the agents and confirmed in the code unless marked.

| Gap | Where |
|---|---|
| The plugin run endpoint never asks the permission rules; it trusts the caller's "approved" flag | `api.rs` `run_plugin_command` |
| Running a tool through the API takes any folder path the caller sends (the permission rules are asked, the folder is not checked) | `api.rs` `execute_tool` |
| Stop does not stop a command that is already running; it runs until its own timeout (up to 30 minutes) | `agent_runner.rs` `execute_local_tool`, `terminal::run` |
| The run list forgets the oldest run after 20, even while it is still working | `agent_runner.rs` `AgentRegistry::insert` |
| `git commit` runs the repository's own hook programs, so it is really a command; it is rated moderate | `tools.rs` git_commit (agent finding, code confirmed) |
| Opening a path can start a program (a .bat or .exe written by the agent), rated moderate | `tools.rs` open_path (agent finding) |
| Page preview can read any local web service on the PC, not only the project's own server | `preview.rs` (agent finding) |
| Fetching a web page on the server accepts any address, including internal ones | `search.rs` `extract_page` |
| The settings endpoint sends the web search key to the browser in plain text | `api.rs` `get_settings` |
| A conversation export carries the whole program's log, not only that conversation | `api.rs` `export_conversation` |
| Two attachments with the same name in one conversation overwrite each other on disk | `api.rs` `add_attachment` |
| The tool record says "approved" for every action that ran, without saying who or how (by mode, by the user, by a session grant) | `agent_runner.rs` `audit_tool` |
| Commands start without the Windows "no window" flag; inside a desktop app each would flash a console window | `terminal.rs` (agent finding, code confirmed) |
| Background commands are not stopped when the program crashes or closes outside a run's end | `terminal.rs` (agent finding) |

## Checked by hand

Each large entry and each surprise was read against the code it cites. Three claims were corrected:

- "The agent loop runs every tool with approval forced on" is not a bug today: the loop asks the
  permission rules itself just before (`decide_call`, `agent_runner.rs` 3756) and passes that decision
  on. It matters for the design only because the desktop app must make that decision itself.
- "Running a tool through the API runs any tool in any folder": the permission rules are asked; only
  the folder is unchecked (corrected in the table above).
- "Starting a model copy can kill its sibling copies": only copies whose parent process has gone.

Phase 0 checks on this copy (2026-10-06): backend 521 passed, 7 skipped on purpose; frontend 114
passed; frontend build passes.
