# Companion Enterprise

The company version of the [Local LLM PC Companion](https://github.com/Insomniac-Coder/local-llm-companion),
in progress: many users and models, servers that scale, local models or OpenAI / Anthropic / Google
behind one gateway, document search, MCP plugins, Jira and Outlook, guards and an audit log, and a
desktop app. The plan and the architecture draft are in `docs/enterprise/` (start with
`Companion-Enterprise-Draft-Plan.pdf`).

Until the first enterprise phase lands, the code is the Companion as forked on 2026-10-06, and the
instructions below run it the way the Companion runs: one person, one PC.

A privacy-first AI assistant that runs entirely on your PC: chat, code questions and agent tasks on
local GGUF models, served by [llama.cpp](https://github.com/ggml-org/llama.cpp). No cloud API key is
needed and nothing leaves your machine unless you turn on web search.

The llama.cpp runtime is **part of this project**: it is built from a pinned upstream commit with the
backends your PC can use (CPU always; Vulkan and CUDA when the hardware and SDKs are present). Model
weights are not included. So is the database: the history is kept in PostgreSQL, and on a PC Companion
runs its own private one (see "The database").

Windows and Linux are both supported: every script and the backend run on either.

More documentation: `docs/ARCHITECTURE.md`, `docs/PERFORMANCE.md`, and the audit records in
`docs/validation/` and `docs/research/`.

## Layout

```
companion-enterprise/
  backend/    Rust (Axum) API: chat, models, tools, agent, search, vision, documents, memory
  frontend/   React + TypeScript + Vite UI, served by the backend
  runtime/    llama.cpp.lock.json and postgresql.lock.json (pinned versions); bin/ and pgsql/ are
              built or installed here (not in Git)
  scripts/    build-runtime.ps1 / build-runtime.sh, get-postgres.py, benchmarks, end-to-end checks
  models/     your GGUF models, one folder per model (none are kept in Git); modeldownloader.py
  plugins/    plugin manifests
  docs/       architecture, performance, research and validation records
  run.ps1     build (first run) and start the app on Windows (PowerShell)
  run.bat     the same from Command Prompt
  run.sh      the same on Linux and macOS
```

## 1. Prerequisites

### Windows

Install these, then open a **new** PowerShell so PATH changes take effect.

| Tool | Needed for | Notes |
| --- | --- | --- |
| [Git](https://git-scm.com/downloads/win) | cloning; fetching llama.cpp | |
| [Node.js 24 LTS](https://nodejs.org/en/download) with npm | the UI and its tests | tests import TypeScript directly (Node 24+) |
| [Rust](https://rust-lang.org/tools/install/) (stable, MSVC toolchain) | the backend | `rustup update stable` if already installed |
| [Visual Studio 2022 or Build Tools](https://visualstudio.microsoft.com/downloads/) with **Desktop development with C++** | the backend and llama.cpp | provides the compiler, Windows SDK, CMake and Ninja |
| **C++ Clang tools for Windows** (a Visual Studio Installer component: *C++ Clang Compiler for Windows*) | *recommended*: faster CPU inference | the runtime build uses clang automatically when it is installed; see the note below |
| [Vulkan SDK](https://vulkan.lunarg.com/sdk/home) | *optional*: the Vulkan GPU backend | any GPU vendor, including integrated GPUs |
| [CUDA Toolkit](https://developer.nvidia.com/cuda-downloads) 12 or 13 | *optional*: the CUDA backend | NVIDIA GPUs only; the display driver is separate and not required from this installer |
| [Python 3](https://www.python.org/downloads/) | the private database install (`scripts/get-postgres.py`); `models/modeldownloader.py` | standard library only |

Check:

```powershell
git --version; node --version; npm --version; rustc --version; cargo --version
```

**Why clang is preferred.** llama.cpp's own Windows releases build the CPU modules with clang. Built
with Microsoft's compiler (MSVC) from the same commit, CPU prompt processing measured 7% slower
(generation speed and GPU speed were the same). When Visual Studio's clang component is installed,
`scripts/build-runtime.ps1` builds the way the official release does: clang builds the CPU modules
and the tools, and MSVC builds only the CUDA and Vulkan modules (NVIDIA supports only MSVC as CUDA's
host compiler on Windows). Without clang, MSVC builds everything. `runtime/bin/BUILD_INFO.json`
records which compiler built which part. If you add clang later, run the build script again.

### Linux / macOS

`git`, `cmake` 3.21+, a C/C++ compiler (`ninja` is used when present), Rust, Node.js 24+, Python 3, and
`make`, `bison` and `flex` for the private database, which is built from source here (Debian/Ubuntu:
`sudo apt install build-essential cmake bison flex python3`). Checked on Ubuntu; macOS is untested.
Optional: the Vulkan SDK or `libvulkan-dev` + `glslc` (Vulkan), the CUDA Toolkit (CUDA, Linux).
macOS builds Metal automatically. Start the app with `run.sh` (see "Run" below).

## 2. Get the project

```powershell
git clone https://github.com/Insomniac-Coder/companion-enterpirse.git companion-enterprise
cd companion-enterprise
```

Run the remaining commands from this directory.

## 3. Build the llama.cpp runtime

The runtime is built once into `runtime/bin/` from the commit in `runtime/llama.cpp.lock.json`.
`run.ps1` (Windows) and `run.sh` (Linux, macOS) do this automatically the first time, or run it yourself:

```powershell
.\scripts\build-runtime.ps1                         # Windows: CPU + every GPU backend this PC can use
.\scripts\build-runtime.ps1 -Backends cpu           # CPU only
.\scripts\build-runtime.ps1 -Backends cpu,cuda -CudaArchitectures 120   # smaller CUDA build for one GPU generation
```

```bash
scripts/build-runtime.sh                            # Linux / macOS
scripts/build-runtime.sh --dry-run                  # print the plan, build nothing
```

What the automatic build (`auto`, the default) chooses:

- **CPU**: always, as one module per x86 instruction set; the fastest one your processor supports is
  loaded at startup.
- **Vulkan**: when the PC has a GPU **and** the Vulkan SDK is installed.
- **CUDA**: when the PC has an **NVIDIA GPU** **and** the CUDA Toolkit is installed. On a PC without
  an NVIDIA GPU the CUDA backend and its runtime libraries are not built or copied at all, even if a
  toolkit is installed. (Asking for `-Backends cuda` explicitly still builds it, with a warning, for
  preparing a runtime for another PC.)
- The CUDA runtime libraries (cuBLAS, cudart, nvJitLink) are copied next to the build, so the PC that
  runs it needs only the NVIDIA display driver, not the toolkit.

The build takes about 10 minutes with CUDA on a fast desktop CPU (much less without it). It fetches
llama.cpp into `build/llama.cpp/`, builds there, then replaces `runtime/bin/` in one step and writes
`runtime/bin/BUILD_INFO.json` (commit, backends, compiler, modules). Rebuild after changing
`runtime/llama.cpp.lock.json` or installing a GPU SDK.

Check the result:

```powershell
.\runtime\bin\llama-server.exe --version
.\runtime\bin\llama-server.exe --list-devices     # CUDA0 / Vulkan0 lines when those backends work
```

```bash
runtime/bin/llama-server --version                  # Linux / macOS
runtime/bin/llama-server --list-devices
```

To use a llama.cpp you built or installed elsewhere, set `COMPANION_LLAMA_SERVER_BIN` to its
`llama-server` executable; the app looks there first, then `runtime/bin/`, then `PATH`.

## 4. The database

Companion keeps conversations, settings and records in PostgreSQL. Unless `COMPANION_DATABASE_URL`
names a database, it runs its own private one: PostgreSQL from `runtime/pgsql/`, its files in
`data/postgres/`, reachable only from this PC (127.0.0.1) with a password generated on first start
(`data/postgres.secret`). It starts with Companion and stops when Companion closes.

`run.ps1`, `run.bat` and `run.sh` install it the first time, or run it yourself:

```bash
python scripts/get-postgres.py      # python3 on Linux; py -3 also works on Windows
```

It installs the version pinned in `runtime/postgresql.lock.json` and refuses any download whose
SHA-256 differs from the one recorded there. On Windows it downloads the official EnterpriseDB build
(385 MB, of which 141 MB is kept); on Linux it builds the same version from the official source (a
22 MB download, a few minutes, 27 MB installed). The download is deleted afterwards.

A server uses its own PostgreSQL instead: set `COMPANION_DATABASE_URL`
(`postgres://user:password@host:5432/name`), and Companion creates its tables on first start.

History from a Local LLM PC Companion (its `data/companion.db`) is copied in once, in order; running
it again copies nothing twice:

```bash
cd backend && cargo run --bin companion-backend -- import-sqlite path/to/companion.db
```

## 5. Add models

Model weights are not kept in this repository. Put each model in its own folder under `models/`, e.g. `models/my-model/my-model-Q4_K_M.gguf`.
Split GGUF files need all their parts in the same folder; a vision model's projector (`mmproj-*.gguf`)
goes in the same folder as its weights.

The included downloader does this for you, verifies the file against the size and SHA-256 the source
publishes, and checks that the runtime can load it:

```powershell
python models\modeldownloader.py hf <owner>/<repository> <file>.gguf
python models\modeldownloader.py hf <owner>/<repository> <file>.gguf --mmproj <projector>.gguf
python models\modeldownloader.py ollama <model>:<tag>
```

On Linux and macOS use `python3 models/modeldownloader.py` with the same arguments.

Prefer the upstream GGUF from Hugging Face. Ollama's own registry files are packaged for Ollama (it
repairs some of them in memory at load), so some load in Ollama but not in stock llama.cpp; the
downloader then keeps the file with an `.incompatible` suffix and writes the reason next to it.

Files added while the app is open appear after **Scan** in the model library. Deleted model folders
disappear from the list on the next refresh.

## 6. Run

### Windows

```powershell
.\run.ps1
```

Or, from Command Prompt (for example when PowerShell's execution policy blocks `run.ps1`):

```bat
run.bat
```

Then open <http://localhost:5173>. The first run builds the runtime if `runtime/bin/llama-server.exe`
is missing (skipped when `COMPANION_LLAMA_SERVER_BIN` is set), installs the private database if
`runtime/pgsql/` is missing (skipped when `COMPANION_DATABASE_URL` is set), installs UI dependencies,
builds the UI and starts the backend; later runs start in seconds. Keep the terminal open and press **Ctrl+C** once
to stop the app and the model server cleanly.

### Linux / macOS

```bash
./run.sh
```

The same steps as `run.ps1`: it builds the runtime with `scripts/build-runtime.sh` and installs the
private database with `scripts/get-postgres.py` the first time, installs the UI dependencies, builds the
UI and starts the backend. Then open <http://localhost:5173>, and press
**Ctrl+C** once in the terminal to stop. If the shell reports `Permission denied`, run
`chmod +x run.sh scripts/build-runtime.sh` once, or start it with `bash run.sh`.

Use a separate clone for each system. The UI dependencies and the runtime are built for the system
that installed them, so one folder shared between Windows and WSL cannot run both `run.ps1` and
`run.sh` (the runtime build refuses to replace a Windows runtime).

### By hand (Linux / macOS)

```bash
scripts/build-runtime.sh                 # once
python3 scripts/get-postgres.py          # once
cd frontend && npm ci && npm run build && cd ..
cd backend && COMPANION_ADDR=127.0.0.1:5173 cargo run --release --bin companion-backend
```

For UI development with hot reload, start the backend on its default port (3877) and run
`npm run dev` in `frontend/` (Vite proxies `/api` to it; set `COMPANION_API_TARGET` to proxy to
another backend).

### First steps in the app

1. **Models**: select a model and **Load**. The app sizes the context and places the model on your
   hardware automatically (see below).
2. **Chat** for conversation and attachments; **Code** for questions about a linked project folder,
   plans and agent tasks.
3. **Settings > Performance** to choose how hardware is used.

## Performance on your hardware

Settings > Performance offers **Auto**, **Fastest**, **Balanced**, **Light** and **Manual**:

- **Auto** (default) chooses for each model at load, aiming for the highest output speed: every layer
  on the GPU if any cache precision (f16 or 8-bit) and GPU memory reserve achieves it, otherwise as
  much of the model on the GPU as fits. Mixture-of-experts models keep expert weights in RAM when the
  GPU is too small, which is much faster than splitting whole layers. The runtime's own memory fit
  decides, so it is exact for every architecture llama.cpp loads.
- **Fastest / Balanced / Light** apply a profile measured for that model on your PC: open the model's
  details on the Models page and choose **Calibrate** (it unloads the current model and takes a minute
  or two). Balanced keeps nearly the fastest generation with fewer cores busy; Light leaves the most
  room for other programs.
- **Manual** sets threads, GPU layers, batch size, cache and waiting behaviour yourself. Combinations
  that cannot work (for example an 8-bit cache with Flash Attention off) cannot be selected.

### How models use tools

The first time a model is loaded, the app checks how it calls tools, which takes a few seconds (you
are told before the load starts). It offers the model a listing tool and a file write, and records
what worked in `tooling.json` next to the model:

- **Tools: native**: the runtime hands the tools to the model in the model's own format and reads its
  calls back, so no model-specific syntax is involved.
- **Tools: text format**: the native calls did not work, so the app's own text action format is used.
- **Read-only code**: file text did not arrive intact in either format (or no format worked). Code
  sessions with this model can read and search but not change files, so Accept edits, Plan and Auto
  are unavailable.

The check runs again automatically when the runtime is rebuilt or the model's chat template changes,
and on request with **Check again** in the model's details (the model must be loaded). The result is
specific to this PC and is never committed to Git.

### Finding and changing code

A model is given more than "read the whole file": `outline` lists what a file
defines with the line numbers, or what a folder holds; `replace_lines` changes
a range of lines by those numbers (the way you would say "replace lines 40 to
52"), which local models get right far more often than reproducing the existing
text exactly; and `project_check` finds the project's own build and test
commands, runs them, and reports the errors as `file:line: message` instead of
pages of output. If the project needs a toolchain this PC does not have, it
says so instead of running something that cannot work.

A project can also keep its own instructions in `AGENTS.md` (or `CLAUDE.md`,
`MUSE.md`, `PROJECT.md`): its commands, its conventions, what not to touch.
Code sessions read the first one they find. It guides the work; it cannot
change what needs your approval.

### Running and looking at what it builds

A command that is meant to keep running - a dev server, a watcher - is started in the background, so
the work carries on around it and its output is kept: the agent starts it, reads what it printed
(including the address it is serving on), and it is stopped when the task ends. Anything else runs
with a timeout, and a command stopped at its timeout still reports what it printed up to then.

With a server running, the agent can open the page: the app loads it in the browser already installed
on this PC, with no window, and reports what the page renders, what it logged, and whether it came up
blank. Only addresses on this PC are opened; nothing is sent anywhere. Models with a context window of
16K or more are offered this, and are asked to keep `HANDOFF.md` and `MEMORY.md` in the project so the
next session picks up where the last one stopped.

A model whose chat template has no tool support creates plain-text documents in chat (`.txt`, `.md`,
`.csv`, `.html`, `.json`) instead of Word, PowerPoint, Excel or PDF files.

### PCs without a GPU

Build the runtime as usual; it contains only the CPU backend. Auto mode loads models on the CPU with
all physical cores and a context cap suited to the free RAM. Prefer 4B–8B models at Q4_K_M, or a
mixture-of-experts model with few active parameters, and leave Reasoning off unless you need it.
If a GPU start fails on a PC that has one, the app retries once on the CPU.

## Working modes

- **Chat** for conversation and attachments.
- **Code / Ask** for read-only project questions (default).
- **Code / Plan** for inspection and a proposed implementation plan without edits.
- **Code / Agent** for changes and verification. **Ask** requests action approvals; explicitly
  selecting **Auto** permits registered actions, including commands and deletion, without per-action
  prompts. File boundaries and tool safety limits still apply; shell commands are not process-sandboxed
  by their working directory. Web search requires the task's Search switch and is blocked when its
  saved policy is Deny.
- **Ctrl+K** finds a session or action. The inspector can be resized by dragging its edge or with the
  arrow keys on its separator.

Code questions can use up to 24 read-only actions per reply (search, read any line range, list
directories); results carry evidence IDs the model can keep or release as context fills.

## Storage, privacy and configuration

- Conversations, settings and records live in PostgreSQL: the private database (its files in
  `data/postgres/`) unless `COMPANION_DATABASE_URL` names another. Attachments, artifacts and logs live
  in `data/` (or an existing `backend/data/`). Set `COMPANION_DATA_DIR` to choose a directory
  explicitly. Back up before moving an installation: `cargo run --bin companion-backend -- backup` in
  `backend/` writes the whole database to `data/backups/<time>.dump` (restore it with `pg_restore`).
- **Settings > Privacy & boundaries > Keep a record of model requests** (on by default) stores what was
  sent to the model and what it returned, capped at the latest 300 requests, for diagnosing wrong or
  broken answers. The records stay on this PC and can contain file contents the assistant read.
- **Save logs as a zip** (Runtime & diagnostics, or the session menu) collects the program's own logs and
  a note of what was running, to share when something failed. Conversations are not in it; conversation
  export was removed on 2026-10-06 (the enterprise audit records hold who asked what).

| Variable | Purpose | Default |
| --- | --- | --- |
| `COMPANION_ADDR` | address the backend listens on | `127.0.0.1:3877` (`run.ps1` and `run.sh` use `:5173`) |
| `COMPANION_DATA_DIR` | data directory | `data/` |
| `COMPANION_DATABASE_URL` | the PostgreSQL database to use (`postgres://user:password@host:5432/name`) | the private database in `data/postgres/` |
| `COMPANION_MODELS_DIR` | models directory | `models/` |
| `COMPANION_FRONTEND_DIR` | compiled UI | `frontend/dist/` |
| `COMPANION_LLAMA_SERVER_BIN` | a specific `llama-server` | `runtime/bin/`, then `PATH` |
| `COMPANION_OIDC_ISSUER`, `COMPANION_OIDC_CLIENT_ID`, `COMPANION_OIDC_CLIENT_SECRET`, `COMPANION_PUBLIC_URL` | sign-in for a server several people use (see "Sign-in") | unset: no sign-in, this PC only |

## Sign-in (a server for several people)

Without sign-in, Companion serves only the PC it runs on: it refuses to listen on an address other
machines can reach, and whoever uses it is the one local person. A server for several people needs
sign-in through the company's identity provider (OpenID Connect: Microsoft Entra ID, or any standard
provider). Set all four (setting some but not all stops startup, naming the missing ones):

| Variable | What it is |
| --- | --- |
| `COMPANION_OIDC_ISSUER` | the identity provider; for Entra ID `https://login.microsoftonline.com/<Directory (tenant) ID>/v2.0` |
| `COMPANION_OIDC_CLIENT_ID` | the app registration's *Application (client) ID* |
| `COMPANION_OIDC_CLIENT_SECRET` | the app registration's client secret; keep it out of Git and shared files |
| `COMPANION_PUBLIC_URL` | the address people open, e.g. `https://companion.example.com` |

Microsoft Entra ID, once, in the Entra admin center (an administrator may need to do this):

1. **App registrations > New registration**: name it Companion; *Accounts in this organizational
   directory only*; redirect URI of type *Web*: `<COMPANION_PUBLIC_URL>/api/auth/callback`.
2. Its **Overview** shows the *Application (client) ID* and the *Directory (tenant) ID*.
3. **Certificates & secrets > New client secret**: its *Value* is the client secret.

Then set `COMPANION_ADDR` to an address other machines reach (for example `0.0.0.0:3877`) and put
Companion behind HTTPS (a reverse proxy such as nginx or IIS, or a cloud load balancer);
`COMPANION_PUBLIC_URL` is that HTTPS address.

People sign in with their company account; a sign-in lasts 12 hours. **Settings > Account** shows who
is signed in, signs out, and makes **API keys** for software such as a build server or a script: it
sends `Authorization: Bearer <key>` and acts as the person who made the key. A key is shown once and
only its fingerprint is stored; withdrawing it stops it at once. Keys are made and withdrawn from a
signed-in browser only, never with another key.

## Checks

```powershell
cd backend;  cargo test
cd ..\frontend;  npm test;  npm run build
```

```bash
cd backend && cargo test
cd ../frontend && npm test && npm run build
```

The backend tests use a throwaway private database in `backend/target/test-postgres/` (PostgreSQL
installed with `scripts/get-postgres.py`), started for the run and stopped after it. Or set
`COMPANION_TEST_DATABASE_URL` to a PostgreSQL database they may create and drop schemas in.

## Troubleshooting

- **`cargo`, `node` or `npm` not recognised**: finish installing and open a new terminal.
- **`link.exe`, MSVC or Windows SDK errors**: install Visual Studio's *Desktop development with C++*
  workload, then use a new terminal.
- **The runtime build skips CUDA or Vulkan**: the build prints why (no matching GPU, or the SDK was not
  found). Install the SDK, open a new terminal so its environment variables are visible, and run the
  build again.
- **`llama-server` not found**: run `scripts\build-runtime.ps1` (Windows) or `scripts/build-runtime.sh`
  (Linux, macOS), or set `COMPANION_LLAMA_SERVER_BIN`.
- **A model does not load**: the error names the cause (for example a file packaged for Ollama, or an
  architecture this llama.cpp version does not know). Check the model's folder for `INCOMPATIBLE.txt`
  when it came from the downloader.
- **No models listed**: models must be `.gguf` files (not archives) inside `models/`; press Scan.
- **Port 5173 or 3877 in use**: stop the previous instance with Ctrl+C in its terminal.
- **Something stopped and you want to know why**: the `logs` folder inside the data folder
  (`backend/data/logs` by default, or `COMPANION_DATA_DIR/logs`) keeps `companion.log` and
  `model-server.log`. They survive restarts; the model server's file shows how it ended.
- **"the private database is not available"**: the message says why. For "PostgreSQL is not
  installed", run `python scripts/get-postgres.py` (`python3` on Linux); otherwise `postgres.log` in the
  logs folder has the database's own account.
- **PowerShell blocks scripts**: follow your organisation's execution policy; do not disable security
  policies globally to run the launcher.

## License

This project is public domain under [The Unlicense](LICENSE). Anyone may clone, fork, copy, modify, publish,
compile, sell or distribute it, for any purpose, commercial or not, with no conditions and no attribution
required.

The third-party software it builds on keeps its own licenses. The llama.cpp runtime (MIT) is built into
`runtime/` by the build script and is not stored in this repository. The Rust and npm dependencies are
fetched at build time. Models you download come with their own terms.
