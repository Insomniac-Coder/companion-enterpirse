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

Found by the agents and confirmed in the code. "Fixed" means fixed in this repository on 2026-10-06
(backend 530 passed, 7 skipped on purpose). New tests cover Stop, background commands, the preview
grant, commit and open asking every time, the delete fix, the run list, the search key, same-name
attachments, saved projects and how a call was allowed; the plugin endpoint's existing test now runs
through the shared gate.

| Gap | Where | Status |
|---|---|---|
| The plugin run endpoint never asked the permission rules; it trusted the caller's "approved" flag | `api.rs` `run_plugin_command` | Fixed: plugins go through the same permission gate as every direct tool call (`gate_tool_call`) |
| Running a tool through the API took any folder path the caller sent | `api.rs` `execute_tool` | Fixed: only a saved project, or a folder inside one (`saved_project_folder`) |
| Stop did not stop a command that was already running; it ran until its own timeout (up to 30 minutes) | `agent_runner.rs`, `terminal.rs`, `project_check.rs` | Fixed: a running command or project check watches the run's stop flag and its whole process tree is ended |
| The run list forgot the oldest run after 20, even while it was still working | `AgentRegistry::insert` | Fixed: only finished runs are forgotten |
| `git commit` runs the repository's own hook programs, so it is really a command; it was rated moderate | `tools.rs` | Fixed: rated like a command (asks every time, never an "Allow for session") |
| Opening a path could start a program (a .bat or .exe the agent wrote), rated moderate | `tools.rs` | Fixed: rated like a command |
| "Allow for session" on a page preview covered every local web service on the PC | `permissions.rs` | Fixed: the grant covers only the port it was given for |
| Fetching a web page on the server accepted any address, including internal ones | `search.rs` `extract_page` | Fixed: the function was never called, and is removed |
| The settings endpoint sent the web search key to the browser in plain text | `api.rs` `get_settings` | Fixed: the screens get "(saved)"; saving that back keeps the key |
| A conversation export carried the whole program's log | `api.rs` `export_conversation` | Fixed (owner decision): conversation export is removed, since the audit records hold who asked what; "Save logs as a zip" shares the program's own logs |
| Two attachments with the same name in one conversation overwrote each other on disk | `api.rs` `add_attachment` | Fixed: the second is kept as "name (2).ext" |
| The tool record said "approved" for every action that ran, without saying how | `agent_runner.rs`, `api.rs` | Fixed: each record says how it was allowed (permission mode, session grant, the user's approval, chat's read-only set) or why it was refused |
| Commands started without the Windows "no window" flag | `terminal.rs`, `tools.rs` (git), `cdp.rs` | Fixed |
| Background commands outlived a crashed or closed program | `terminal.rs` | Fixed on Windows (they belong to a job the system ends with the program) and on every system for a normal close; a crash on Linux or macOS is left to the desktop app (Phase 3) |
| A delete that named a line (`{"path": ..., "line": 12}`) deleted the whole file, permanently (owner report, 2026-10-06) | `tools.rs` `delete_file` | Fixed: any argument besides the path is refused and nothing is deleted; the message points to the line tools |
| Tool ratings were kept in two lists that could disagree | `tools.rs` `registry`, `risk_of` | Fixed: the tool list reads the one rating the permission gate uses |

## Checked by hand

Each large entry and each surprise was read against the code it cites. Three claims were corrected:

- "The agent loop runs every tool with approval forced on" is not a bug today: the loop asks the
  permission rules itself just before (`decide_call`, `agent_runner.rs` 3756) and passes that decision
  on. It matters for the design only because the desktop app must make that decision itself.
- "Running a tool through the API runs any tool in any folder": the permission rules are asked; only
  the folder is unchecked (corrected in the table above).
- "Starting a model copy can kill its sibling copies": only copies whose parent process has gone.

Phase 0 checks on this copy (2026-10-06): backend 521 passed, 7 skipped on purpose; frontend 114
passed; frontend build passes. After the fixes above: backend 530 passed, 7 skipped.
