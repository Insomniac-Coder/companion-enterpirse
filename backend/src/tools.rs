//! Tool system (§23–§24, §60): schemas, risk levels, validated handlers.
//!
//! Pipeline: LLM -> structured ToolRequest -> registry lookup -> workspace +
//! permission validation -> handler -> ToolResult -> model (§92).

use crate::permissions::RiskLevel;
use crate::workspace::{WorkspaceError, WorkspaceManager};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub risk: RiskLevel,
    pub permission_required: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub name: String,
    pub args: serde_json::Value,
    /// Set once the user approves in the permission UX (§26).
    #[serde(default)]
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub ok: bool,
    pub output: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

impl ToolResult {
    pub fn ok(output: String) -> Self {
        Self {
            ok: true,
            output,
            exit_code: None,
        }
    }
    pub fn fail(output: String) -> Self {
        Self {
            ok: false,
            output,
            exit_code: None,
        }
    }
}

#[derive(Debug)]
pub enum ToolError {
    UnknownTool(String),
    InvalidArgs(String),
    PermissionRequired { tool: String, reason: String },
    Workspace(WorkspaceError),
    Io(std::io::Error),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTool(t) => write!(f, "Unknown tool '{t}'."),
            Self::InvalidArgs(m) => write!(f, "Invalid tool arguments: {m}"),
            Self::PermissionRequired { tool, reason } => {
                write!(
                    f,
                    "Permission required for '{tool}': {reason} [Allow Once] [Deny]"
                )
            }
            Self::Workspace(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "Tool I/O error: {e}"),
        }
    }
}

pub fn registry() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "list_directory",
            description: "List files in a workspace directory",
            risk: RiskLevel::Safe,
            permission_required: "workspace read",
        },
        ToolDescriptor {
            name: "read_file",
            description: "Read a numbered text chunk: path, optional 1-based start_line/end_line (200 lines default, 500 max). Any line is accessible; follow the returned continuation for more. Optional start_column continues an unusually long line.",
            risk: RiskLevel::Safe,
            permission_required: "workspace read",
        },
        ToolDescriptor {
            name: "write_file",
            description: "Create or overwrite a workspace file",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "append_file",
            description: "Add text to the end of a workspace file, creating it if absent",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "edit_file",
            description: "Replace exact existing text: old (copied from the file, without the line-number labels) becomes new and must occur once. replace_lines is easier when you know the line numbers.",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "replace_lines",
            description: "Replace lines by number, as read_file shows them",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "outline",
            description: "What a file defines, or what a folder holds",
            risk: RiskLevel::Safe,
            permission_required: "none",
        },
        ToolDescriptor {
            name: "project_check",
            description: "Run this project's own build and tests and report what failed",
            risk: RiskLevel::Dangerous,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "delete_file",
            description: "Delete a workspace file",
            risk: RiskLevel::Dangerous,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "search_text",
            description: "Regex search in a workspace directory or individual file. Returns matching line numbers; use read_file start_line/end_line for surrounding code. Narrow path/query when results are capped.",
            risk: RiskLevel::Safe,
            permission_required: "workspace read",
        },
        ToolDescriptor {
            name: "execute_command",
            description: "Run a shell command with captured output",
            risk: RiskLevel::Dangerous,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "changes",
            description: "Every file this task has created, changed or deleted, with its size now",
            risk: RiskLevel::Safe,
            permission_required: "none",
        },
        ToolDescriptor {
            name: "remember",
            description: "Keep one short fact in front of you (it survives summarizing)",
            risk: RiskLevel::Safe,
            permission_required: "none",
        },
        ToolDescriptor {
            name: "preview_page",
            description: "Open a page this project serves; reports what it renders and logs",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "web_search",
            description: "Search the web (explicit opt-in per request)",
            risk: RiskLevel::Moderate,
            permission_required: "explicit (Search toggle)",
        },
        ToolDescriptor {
            name: "create_document",
            description: "Generate txt/md/json/csv/html/xlsx/docx/pdf/pptx from a JSON spec",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "system_info",
            description: "OS/CPU/RAM/GPU summary",
            risk: RiskLevel::Safe,
            permission_required: "none",
        },
        ToolDescriptor {
            name: "git_commit",
            description: "Commit staged workspace changes with a message",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
        ToolDescriptor {
            name: "list_processes",
            description: "List running OS processes (name + pid)",
            risk: RiskLevel::Safe,
            permission_required: "none",
        },
        ToolDescriptor {
            name: "present_plan",
            description: "Plan runs only: present the finished plan for the user to approve. Ends the run.",
            risk: RiskLevel::Safe,
            permission_required: "none",
        },
        ToolDescriptor {
            name: "open_path",
            description: "Open a workspace file/folder with the OS default app",
            risk: RiskLevel::Moderate,
            permission_required: "explicit",
        },
    ]
}

pub fn risk_of(name: &str) -> RiskLevel {
    match name {
        "execute_command" | "delete_file" => RiskLevel::Dangerous,
        "project_check" => RiskLevel::Dangerous,
        "write_file" | "append_file" | "edit_file" | "replace_lines" | "web_search"
        | "create_document" | "git_commit" | "open_path" | "preview_page" => RiskLevel::Moderate,
        _ => RiskLevel::Safe,
    }
}

/// Tools the run itself carries out rather than the registry: a web search
/// needs the async request path and its own consent, and a kept note belongs
/// to the run's own context. `execute` refuses both by name.
pub fn runs_in_the_agent(name: &str) -> bool {
    matches!(name, "web_search" | "remember" | "changes" | "preview_page")
}

/// Tools that write files inside the project: what Accept edits mode runs
/// without asking. Deletion is not an edit.
/// The note the host leaves in place of file text it released from a model's
/// working context. A model that sees its own earlier write rewritten this
/// way can send the note back as the file's content: two components were
/// written as 169-byte files holding this sentence, and the next run spent
/// four steps trying to understand them (night decision 61).
pub const RELEASED_NOTE: &str = "[Released:";

/// Whether this text is the host's own release note rather than file text.
pub fn is_released_note(text: &str) -> bool {
    text.trim_start().starts_with(RELEASED_NOTE)
}

fn reject_released_note(tool: &str, text: &str) -> Result<(), ToolError> {
    if is_released_note(text) {
        return Err(ToolError::InvalidArgs(format!(
            "{tool} was given the host's context note instead of file text. That note replaced text released from your working context; it is not the file. Send the complete text the file should hold, or read the file first if you need what is in it."
        )));
    }
    Ok(())
}

pub fn is_file_edit(tool: &str) -> bool {
    matches!(tool, "write_file" | "append_file" | "edit_file" | "replace_lines" | "create_document")
}

/// A file edit inside a `.git` folder. Git runs commands named there (hooks,
/// `core.fsmonitor`, `diff.external` in its config), so such an edit is never
/// automatic outside Auto: review found that Accept edits would otherwise let
/// a model plant a command that the app's own `git status` later runs.
pub fn touches_git_internals(tool: &str, args: &serde_json::Value) -> bool {
    is_file_edit(tool)
        && args
            .get("path")
            .and_then(|path| path.as_str())
            .is_some_and(|path| path.replace('\\', "/").split('/').any(|part| part.eq_ignore_ascii_case(".git")))
}

/// Arguments a tool cannot run without, and whether each must be non-empty.
/// The schema-constrained action envelope builds its grammar from these, so a
/// model under it cannot leave one out. Optional arguments are not listed.
pub fn required_args(tool: &str) -> &'static [(&'static str, bool)] {
    match tool {
        "read_file" | "delete_file" => &[("path", true)],
        // May be empty: the runner then presents the plan the model just wrote.
        "present_plan" => &[("plan", false)],
        "write_file" | "append_file" => &[("path", true), ("content", false)],
        "edit_file" => &[("path", true), ("old", true), ("new", false)],
        "replace_lines" => &[("path", true), ("from", true), ("to", true), ("text", false)],
        "outline" => &[("path", true)],
        "search_text" => &[("query", true)],
        "execute_command" => &[("command", true)],
        "preview_page" => &[("url", true)],
        "remember" => &[("note", true)],
        "git_commit" => &[("message", true)],
        _ => &[],
    }
}

/// Tool names the in-chat loop may run WITHOUT asking (Stage 31): read-only,
/// workspace-confined, no side effects. Everything else needs the agent
/// panel/commands approval flow.
/// Tools a chat reply may run without approval: read-only inspection, plus
/// `create_document`, which only ever writes into the app's own artifacts
/// folder (never the project) and hands the user a file to open or save.
pub fn chat_safe(name: &str) -> bool {
    matches!(
        name,
        "list_directory"
            | "read_file"
            | "search_text"
            | "outline"
            | "system_info"
            | "list_processes"
            | "create_document"
    )
}

const MAX_FILE_BYTES: u64 = 5_000_000;

/// Marks a write the host read back and found identical to what was sent.
pub const WRITE_VERIFIED: &str = "(verified on disk)";
const MAX_SEARCH_MATCHES: usize = 50;

fn require_approved(req: &ToolRequest, approved: bool, what: &str) -> Result<(), ToolError> {
    if approved || req.approved {
        return Ok(());
    }
    Err(ToolError::PermissionRequired {
        tool: req.name.clone(),
        reason: what.into(),
    })
}

/// A path that does not exist, reported with the project's files or folders of
/// the same name (at most five, workspace-relative). Small models drop the
/// folder part of a path they saw in the listing: measured live, a 4B model
/// asked for `orders.py` three times when the file was `project/orders.py`,
/// and "cannot find the file specified" never told it where to look.
fn missing_path(ws: &WorkspaceManager, rel: &str, error: std::io::Error) -> ToolError {
    if error.kind() != std::io::ErrorKind::NotFound {
        return ToolError::Io(error);
    }
    let Some(name) = std::path::Path::new(rel.trim_end_matches(['/', '\\'])).file_name().map(|name| name.to_os_string()) else {
        return ToolError::Io(error);
    };
    let root = ws.root().to_path_buf();
    let mut matches: Vec<String> = Vec::new();
    let mut pending = vec![(root.clone(), 0usize)];
    let mut seen = 0usize;
    'walk: while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > 20_000 || matches.len() >= 5 {
                break 'walk;
            }
            let entry_name = entry.file_name();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if entry_name.eq_ignore_ascii_case(&name) {
                if let Ok(relative) = entry.path().strip_prefix(&root) {
                    matches.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
            let skipped = entry_name
                .to_str()
                .is_some_and(|dir_name| dir_name.starts_with('.') || matches!(dir_name, "node_modules" | "target" | "__pycache__" | "dist" | "build"));
            if is_dir && !skipped && depth < 8 {
                pending.push((entry.path(), depth + 1));
            }
        }
    }
    matches.sort();
    let hint = if matches.is_empty() {
        "list_directory shows what exists.".to_string()
    } else {
        format!("Found with that name: {}.", matches.join(", "))
    };
    ToolError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("no such path '{rel}' (paths are relative to the project root). {hint}"),
    ))
}

/// Execute a validated tool. SAFE reads run immediately; MODERATE/DANGEROUS
/// require `req.approved == true` (set by the permission UX, §26).
pub fn execute(
    req: &ToolRequest,
    ws: &WorkspaceManager,
    approved: bool,
) -> Result<ToolResult, ToolError> {
    match req.name.as_str() {
        "list_directory" => {
            let rel = req.args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let dir = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if dir.is_file() {
                return Err(ToolError::InvalidArgs(format!(
                    "{rel} is a file, not a folder: read it with read_file"
                )));
            }
            let entries = std::fs::read_dir(&dir).map_err(|error| missing_path(ws, rel, error))?;
            // Folders end in "/": the names alone did not say which could be
            // listed and which read.
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if e.file_type().is_ok_and(|kind| kind.is_dir()) {
                        format!("{name}/")
                    } else {
                        name
                    }
                })
                .collect();
            names.sort();
            if names.is_empty() {
                return Ok(ToolResult::ok(format!("{rel} is an empty folder")));
            }
            Ok(ToolResult::ok(names.join("\n")))
        }
        "read_file" => {
            let rel = req.args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("read_file requires {\"path\": \"...\"}".into())
            })?;
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if p.is_dir() {
                return Err(ToolError::InvalidArgs(format!(
                    "{rel} is a folder, not a file: list it with list_directory"
                )));
            }
            crate::file_read::read(&p, &req.args).map_err(|error| match error {
                ToolError::Io(io) if io.kind() == std::io::ErrorKind::InvalidData => ToolError::InvalidArgs(format!(
                    "{rel} is not a text file (it holds binary data), so it cannot be read as text"
                )),
                ToolError::Io(io) => missing_path(ws, rel, io),
                other => other,
            })
        }
        "write_file" => {
            require_approved(req, approved, "File creation needs explicit approval.")?;
            let rel = req.args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("write_file requires {\"path\": \"...\", \"content\": \"...\"}".into())
            })?;
            let content = req.args.get("content").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("write_file requires {\"path\": \"...\", \"content\": \"...\"}".into())
            })?;
            if content.len() as u64 > MAX_FILE_BYTES {
                return Err(ToolError::InvalidArgs(format!(
                    "content too large ({} bytes, max {MAX_FILE_BYTES})", content.len()
                )));
            }
            reject_released_note("write_file", content)?;
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if p.is_dir() {
                return Err(ToolError::InvalidArgs(format!(
                    "{rel} is a folder: give the path of a file inside it, such as {}/index.html",
                    rel.trim_end_matches(['/', '\\'])
                )));
            }
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
            }
            std::fs::write(&p, content).map_err(ToolError::Io)?;
            // Read the file back: the host can establish byte for byte that
            // the requested content is what is on disk, which is stronger
            // evidence than asking the model to read its own text again (and
            // costs no context on a small window).
            let on_disk = std::fs::read(&p).map_err(ToolError::Io)?;
            if on_disk != content.as_bytes() {
                return Err(ToolError::Io(std::io::Error::other(format!(
                    "{rel} was written but reads back as {} bytes instead of the {} sent; check the path and disk space",
                    on_disk.len(),
                    content.len()
                ))));
            }
            Ok(ToolResult::ok(format!(
                "wrote {} bytes ({} lines) to {rel} {WRITE_VERIFIED}",
                content.len(),
                content.lines().count()
            )))
        }
        // Writing a long file in parts is the only way to produce one at all
        // when a reply cannot hold it: a 5K-token window leaves room for a few
        // thousand characters per step. Appending also costs no context for
        // an anchor, which edit_file needs and can fail to match.
        "append_file" => {
            require_approved(req, approved, "File modifications need explicit approval.")?;
            const USAGE: &str =
                "append_file requires {\"path\": \"...\", \"content\": \"text to add at the end\"}";
            let rel = req
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs(USAGE.into()))?;
            let content = req
                .args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArgs(USAGE.into()))?;
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if p.is_dir() {
                return Err(ToolError::InvalidArgs(format!("{rel} is a folder, not a file")));
            }
            let existing = match std::fs::metadata(&p) {
                Ok(meta) => meta.len(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
                Err(e) => return Err(ToolError::Io(e)),
            };
            if existing + content.len() as u64 > MAX_FILE_BYTES {
                return Err(ToolError::InvalidArgs(format!(
                    "file would exceed {MAX_FILE_BYTES} bytes ({existing} already written)"
                )));
            }
            reject_released_note("append_file", content)?;
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
            }
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .map_err(ToolError::Io)?;
            file.write_all(content.as_bytes()).map_err(ToolError::Io)?;
            file.flush().map_err(ToolError::Io)?;
            drop(file);
            // Same read-back as write_file: the host confirms the bytes landed
            // rather than asking the model to take its own word for it.
            let on_disk = std::fs::metadata(&p).map(|meta| meta.len()).unwrap_or(0);
            let expected = existing + content.len() as u64;
            if on_disk != expected {
                return Err(ToolError::Io(std::io::Error::other(format!(
                    "{rel} reads back as {on_disk} bytes instead of the expected {expected}"
                ))));
            }
            Ok(ToolResult::ok(format!(
                "appended {} bytes ({} lines) to {rel}; it is now {on_disk} bytes {WRITE_VERIFIED}",
                content.len(),
                content.lines().count()
            )))
        }
        "edit_file" => {
            require_approved(req, approved, "File modifications need explicit approval.")?;
            const USAGE: &str = "edit_file requires {\"path\": \"...\", \"old\": \"exact existing text\", \"new\": \"replacement text\"} (or {\"path\", \"patch\"} with a unified diff)";
            let rel = req.args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs(USAGE.into())
            })?;
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if p.is_dir() {
                return Err(ToolError::InvalidArgs(format!("{rel} is a folder, not a file")));
            }
            let original = std::fs::read_to_string(&p).map_err(|error| missing_path(ws, rel, error))?;
            if original.len() as u64 > MAX_FILE_BYTES {
                return Err(ToolError::InvalidArgs("file too large to patch; rewrite it in chunks".into()));
            }
            // Exact-text replacement is what small local models produce
            // reliably: they copy the lines they saw and write the new ones.
            // Unified diffs stay supported for models that emit them well.
            let updated = if let Some(old) = req.args.get("old").and_then(|v| v.as_str()) {
                let new = req.args.get("new").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::InvalidArgs(USAGE.into())
                })?;
                reject_released_note("edit_file", new)?;
                // An edit whose old and new are the same changes nothing. It
                // used to report success, and a model took three such edits
                // for changes it had made (owner's 26B run, 2026-09-18).
                if old.replace("\r\n", "\n") == new.replace("\r\n", "\n") {
                    return Err(ToolError::InvalidArgs(format!(
                        "old and new are the same text, so this would change nothing in {rel}. Put the text the file should hold in new."
                    )));
                }
                // Made already, by an earlier step: repeating a successful
                // edit used to fail as "not found" and read as a new problem,
                // and repeating an insertion used to insert it twice.
                if edit_already_made(&original, old, new) {
                    return Ok(ToolResult::ok(format!(
                        "{rel} already holds the new text: this change was made by an earlier step, so nothing was changed."
                    )));
                }
                let replace_all = req.args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
                apply_replacement(&original, old, new, replace_all).map_err(ToolError::InvalidArgs)?
            } else if let Some(patch) = req.args.get("patch").and_then(|v| v.as_str()) {
                apply_unified_patch(&original, patch).map_err(ToolError::InvalidArgs)?
            } else {
                return Err(ToolError::InvalidArgs(USAGE.into()));
            };
            std::fs::write(&p, &updated).map_err(ToolError::Io)?;
            let on_disk = std::fs::read(&p).map_err(ToolError::Io)?;
            if on_disk != updated.as_bytes() {
                return Err(ToolError::Io(std::io::Error::other(format!(
                    "{rel} was patched but does not read back as written; read it again before the next change"
                ))));
            }
            Ok(ToolResult::ok(format!(
                "patched {rel} ({}): {} -> {} bytes, read back from disk",
                changed_lines(&original, &updated),
                original.len(),
                updated.len()
            )))
        }
        // Editing by line number: a model that has just read a file with its
        // lines labelled has the numbers in front of it, where reproducing the
        // old text byte for byte is what it keeps getting wrong (owner,
        // 2026-09-18; every run stopped that day died on edit_file arguments).
        "replace_lines" => {
            require_approved(req, approved, "File modifications need explicit approval.")?;
            const USAGE: &str = "replace_lines requires {\"path\": \"...\", \"from\": 12, \"to\": 18, \"text\": \"the replacement lines\"} - the line numbers read_file shows, and text may be empty to delete those lines";
            let rel = req.args.get("path").and_then(|v| v.as_str()).ok_or_else(|| ToolError::InvalidArgs(USAGE.into()))?;
            let from = req.args.get("from").and_then(|v| v.as_u64()).ok_or_else(|| ToolError::InvalidArgs(USAGE.into()))? as usize;
            let to = req.args.get("to").and_then(|v| v.as_u64()).map(|to| to as usize).unwrap_or(from);
            let text = req.args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            reject_released_note("replace_lines", text)?;
            if from == 0 {
                return Err(ToolError::InvalidArgs("line numbers start at 1".into()));
            }
            if to < from {
                return Err(ToolError::InvalidArgs(format!("to ({to}) is before from ({from})")));
            }
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            let original = std::fs::read_to_string(&p).map_err(|error| missing_path(ws, rel, error))?;
            let crlf = original.contains("\r\n");
            let lines: Vec<&str> = original.lines().collect();
            if from > lines.len() {
                return Err(ToolError::InvalidArgs(format!(
                    "{rel} has {} lines; from {from} is past its end. Read it again for the current numbers.",
                    lines.len()
                )));
            }
            let last = to.min(lines.len());
            // An optional anchor: the text being replaced, as the model read
            // it - the first line, or every line of the range, which is what
            // models send (5 of 6 calls failed on it, owner's 26B run,
            // 2026-09-18). Line numbers move after every edit, and a wrong
            // number silently destroys the wrong lines.
            if let Some(expect) = req.args.get("expect").and_then(|v| v.as_str()).filter(|text| !text.trim().is_empty()) {
                let mut expected: Vec<&str> = expect.lines().map(str::trim).collect();
                while expected.last().is_some_and(|line| line.is_empty()) {
                    expected.pop();
                }
                let end = (from - 1 + expected.len()).min(lines.len());
                let actual: Vec<&str> = lines[from - 1..end].iter().map(|line| line.trim()).collect();
                if actual != expected {
                    let found = line_block_positions(&lines, &expected);
                    let message = match found.as_slice() {
                        [at] => format!(
                            "{rel} has that text at line {} now, not at line {from}: the numbers have moved. Send from {} and to {} for the same lines.",
                            at + 1,
                            at + 1,
                            at + 1 + (last - from)
                        ),
                        _ => format!(
                            "lines {from}-{end} of {rel} hold:\n{}\nnot the text in expect, and that text is not in the file as it is now. expect is the current text of line {from} (the first line is enough); read the file again if you are not sure what it holds.",
                            numbered_lines(&lines, from - 1, end)
                        ),
                    };
                    return Err(ToolError::InvalidArgs(message));
                }
            }
            let mut updated: Vec<&str> = Vec::with_capacity(lines.len());
            updated.extend_from_slice(&lines[..from - 1]);
            let replacement: Vec<&str> = if text.is_empty() { Vec::new() } else { text.lines().collect() };
            updated.extend_from_slice(&replacement);
            updated.extend_from_slice(&lines[last..]);
            // The file's own line endings: joining with "\n" alone turned a
            // Windows file's every line ending into a Unix one.
            let separator = if crlf { "\r\n" } else { "\n" };
            let mut body = updated.join(separator);
            if original.ends_with('\n') && !body.is_empty() {
                body.push_str(separator);
            }
            if body.len() as u64 > MAX_FILE_BYTES {
                return Err(ToolError::InvalidArgs(format!("file would exceed {MAX_FILE_BYTES} bytes")));
            }
            std::fs::write(&p, &body).map_err(ToolError::Io)?;
            let on_disk = std::fs::read_to_string(&p).map_err(ToolError::Io)?;
            if on_disk != body {
                return Err(ToolError::Io(std::io::Error::other(format!("{rel} did not read back as written"))));
            }
            Ok(ToolResult::ok(format!(
                "replaced lines {from}-{last} of {rel} ({} line(s) -> {} line(s)); it is now {} lines {WRITE_VERIFIED}. Line numbers after {from} have moved: read it again before editing by number.",
                last - from + 1,
                replacement.len(),
                updated.len()
            )))
        }
        "outline" => {
            let rel = req.args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("outline requires {\"path\": \"a file or folder; \\\".\\\" for the workspace root\"}".into())
            })?;
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if !p.exists() {
                return Err(missing_path(ws, rel, std::io::ErrorKind::NotFound.into()));
            }
            crate::outline::outline(&p, rel)
                .map(ToolResult::ok)
                .map_err(ToolError::InvalidArgs)
        }
        // Running the project's own build and tests is running its code, so it
        // carries the same weight as any other command.
        "project_check" => {
            require_approved(req, approved, "Running the project's build and tests needs explicit approval.")?;
            let rel = req.args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let root = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if !root.is_dir() {
                return Err(ToolError::InvalidArgs(format!("{rel} is not a folder")));
            }
            let timeout = req
                .args
                .get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| crate::project_check::default_timeout().as_secs())
                .clamp(5, crate::terminal::MAX_TIMEOUT_SECS);
            Ok(ToolResult::ok(crate::project_check::run(&root, timeout)))
        }
        "delete_file" => {
            require_approved(req, approved, "File deletion needs explicit approval.")?;
            let rel = req.args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("delete_file requires {\"path\": \"...\"}".into())
            })?;
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if !p.exists() {
                return Err(missing_path(ws, rel, std::io::ErrorKind::NotFound.into()));
            }
            if !p.is_file() {
                return Err(ToolError::InvalidArgs(format!("not a file: {rel}")));
            }
            std::fs::remove_file(&p).map_err(ToolError::Io)?;
            Ok(ToolResult::ok(format!("deleted {rel}")))
        }
        "search_text" => {
            let q = req.args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("search_text requires {\"query\": \"...\"}".into())
            })?;
            let rel = req.args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let dir = ws.resolve(rel).map_err(ToolError::Workspace)?;
            if !dir.exists() {
                return Err(missing_path(ws, rel, std::io::ErrorKind::NotFound.into()));
            }
            // Code a model searches for is often not a valid expression -
            // `render(` or `items[0]` - and was refused; it is searched for as
            // it is written instead.
            let (re, note) = match regex::Regex::new(q) {
                Ok(re) => (re, None),
                Err(_) => (
                    regex::Regex::new(&regex::escape(q)).map_err(|e| ToolError::InvalidArgs(format!("bad query: {e}")))?,
                    Some(format!("({q:?} is not a regular expression, so it was searched for as plain text.)\n")),
                ),
            };
            let found = search_dir(&dir, ws.root(), &re);
            Ok(ToolResult::ok(format!("{}{found}", note.unwrap_or_default())))
        }
        "execute_command" => {
            require_approved(req, approved, "Command execution needs explicit approval.")?;
            // A command already started is read or stopped by its id: a dev
            // server keeps running while the work carries on around it.
            if let Some(id) = req.args.get("background_id").and_then(|v| v.as_u64()) {
                let id = id as u32;
                let stopping = req.args.get("stop").and_then(|v| v.as_bool()).unwrap_or(false);
                let status = if stopping {
                    crate::terminal::stop_background(id)
                } else {
                    crate::terminal::background_status(id)
                };
                return match status {
                    Some(status) => Ok(ToolResult::ok(crate::terminal::format_background(&status))),
                    None => {
                        let running = crate::terminal::background_commands();
                        let known = if running.is_empty() {
                            "none is running".to_string()
                        } else {
                            running.iter().map(|(id, cmd)| format!("{id}: {cmd}")).collect::<Vec<_>>().join("; ")
                        };
                        Err(ToolError::InvalidArgs(format!(
                            "no background command {id} ({known}). Start one with {{\"command\": \"...\", \"background\": true}}."
                        )))
                    }
                };
            }
            let cmd = req.args.get("command").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("execute_command requires {\"command\": \"...\"}".into())
            })?;
            let timeout = req.args.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(crate::terminal::DEFAULT_TIMEOUT_SECS);
            let cwd = ws.resolve(req.args.get("cwd").and_then(|v| v.as_str()).unwrap_or("."))
                .map_err(ToolError::Workspace)?;
            if !cwd.is_dir() {
                return Err(ToolError::InvalidArgs("cwd is not a directory".into()));
            }
            // A server run in the foreground holds the whole task still until
            // the timeout and then reports that it was killed. Saying so at
            // once costs a step; letting it run costs two minutes, twice over
            // in one live run (owner watching, 2026-09-18).
            let background = req.args.get("background").and_then(|v| v.as_bool()).unwrap_or(false);
            if !background {
                if let Some(what) = crate::terminal::keeps_running(cmd) {
                    return Err(ToolError::InvalidArgs(format!(
                        "{what:?} does not exit on its own, so running it here would hold this task still until the timeout. Send the same command with \"background\": true; it keeps running, you get its output and the address it prints, and it is stopped when the task ends."
                    )));
                }
            }
            if background {
                // Watched only long enough to print its address or to fail.
                let settle = std::time::Duration::from_secs(
                    req.args.get("wait_secs").and_then(|v| v.as_u64()).unwrap_or(10).clamp(1, 120),
                );
                let owner = ws.root().to_string_lossy().into_owned();
                return match crate::terminal::start_background(cmd, &cwd, &owner, settle) {
                    Ok(status) => {
                        let mut res = ToolResult::ok(crate::terminal::format_background(&status));
                        res.exit_code = status.exit_code;
                        Ok(res)
                    }
                    Err(e) => Err(ToolError::InvalidArgs(e)),
                };
            }
            match crate::terminal::run(cmd, &cwd, timeout) {
                Ok(r) => {
                    let mut output = crate::terminal::format_result(cmd, &r);
                    // A background command stopped as if it were a shell
                    // command ("stop --background_id 1"): measured live, a
                    // small model wrote the stop call that way.
                    let first = cmd.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                    let running = crate::terminal::background_commands();
                    if r.exit_code != Some(0) && matches!(first.as_str(), "stop" | "kill") && !running.is_empty() {
                        let ids = running.iter().map(|(id, cmd)| format!("{id} ({cmd})")).collect::<Vec<_>>().join(", ");
                        output.push_str(&format!(
                            "\n\nA background command is stopped with execute_command itself, not a shell command: send {{\"background_id\": N, \"stop\": true}}. Running now: {ids}."
                        ));
                    }
                    let mut res = ToolResult::ok(output);
                    res.exit_code = r.exit_code;
                    Ok(res)
                }
                Err(e) => Err(ToolError::InvalidArgs(e)),
            }
        }
        "preview_page" => Err(ToolError::InvalidArgs(
            "preview_page is carried out by the run, which knows the conversation its picture belongs to".into(),
        )),
        "system_info" => Ok(ToolResult::ok(format!(
            "os={} arch={}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))),
        // Stage 36: version control with guardrails. Reads run through the
        // shell allowlist; commit is MODERATE (approval). Destructive git
        // (reset --hard, push --force, clean -fd) is refused outright — use
        // the Git panel's explicit confirm flow instead.
        "git_commit" => {
            require_approved(req, approved, "Committing needs explicit approval.")?;
            let msg = req.args.get("message").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::InvalidArgs("git_commit requires {\"message\": \"...\"}".into())
            })?;
            if msg.trim().is_empty() || msg.len() > 500 {
                return Err(ToolError::InvalidArgs("commit message must be 1–500 chars".into()));
            }
            if msg.contains(['\n', '\r', '"', '\'', '`', '$', ';', '&', '|']) {
                return Err(ToolError::InvalidArgs("commit message must be a single plain line".into()));
            }
            let root = ws.root().to_path_buf();
            let status = std::process::Command::new("git")
                .args(["-C", &root.to_string_lossy(), "commit", "-m", msg.trim()])
                .output()
                .map_err(ToolError::Io)?;
            let body = format!("{}{}", String::from_utf8_lossy(&status.stdout), String::from_utf8_lossy(&status.stderr));
            if status.status.success() {
                Ok(ToolResult::ok(body.chars().take(2000).collect()))
            } else {
                Ok(ToolResult { ok: false, output: body.chars().take(2000).collect(), exit_code: status.status.code() })
            }
        }
        // Stage 37: automation primitives. list_processes is read-only (SAFE);
        // open_path launches the OS default handler (MODERATE, approval).
        "list_processes" => {
            let mut sys = sysinfo::System::new_all();
            sys.refresh_processes();
            let mut rows: Vec<String> = sys.processes().values()
                .map(|p| format!("{} (pid {})", p.name(), p.pid()))
                .collect();
            rows.sort();
            rows.truncate(300);
            Ok(ToolResult::ok(rows.join("\n")))
        }
        // The plan run's finish (Claude Code's ExitPlanMode). The agent loop
        // handles it; it does nothing on its own.
        "present_plan" => Err(ToolError::InvalidArgs(
            "present_plan requires {\"plan\": \"...\"} and only ends a plan run".into(),
        )),
        "open_path" => {
            require_approved(req, approved, "Opening apps/files needs explicit approval.")?;
            let rel = req.args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let p = ws.resolve(rel).map_err(ToolError::Workspace)?;
            // The OS handler is fed only paths that exist. Given a missing
            // path, Windows Explorer opens an unrelated folder window over
            // the app and the call would still have looked successful.
            if !p.exists() {
                return Err(ToolError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "nothing exists at {rel} in the workspace, so nothing was opened. open_path opens an existing file or folder of the project; documents made with create_document live in the chat's Artifacts panel, where the user opens them."
                    ),
                )));
            }
            let target = p.to_string_lossy().into_owned();
            let mut cmd = match std::env::consts::OS {
                "windows" => {
                    let mut c = std::process::Command::new("explorer");
                    c.arg(&target);
                    c
                }
                "macos" => {
                    let mut c = std::process::Command::new("open");
                    c.arg(&target);
                    c
                }
                _ => {
                    let mut c = std::process::Command::new("xdg-open");
                    c.arg(&target);
                    c
                }
            };
            match cmd.spawn() {
                Ok(_) => Ok(ToolResult::ok(format!("opened {rel} with the OS default handler"))),
                Err(e) => Err(ToolError::Io(e)),
            }
        }
        // Stage 11: web search needs async HTTP + provider config, so it runs
        // in the async API layer (chat handler / agent loop / tools endpoint),
        // never here. The registry entry above documents it for the model.
        "web_search" => Err(ToolError::InvalidArgs(
            "web_search executes in the async request path with explicit Search consent, not via sync execute()".into(),
        )),
        "remember" => Err(ToolError::InvalidArgs(
            "remember is kept by the run itself, not by the tool registry".into(),
        )),
        "changes" => Err(ToolError::InvalidArgs(
            "changes is answered by the run itself, which is what knows them".into(),
        )),
        other => Err(ToolError::UnknownTool(other.into())),
    }
}

/// Apply a unified diff to `original` (§31: structured patches, not rewrites).
/// Supports multiple `@@` hunks with context verification (fuzz 0). The
/// `---`/`+++` headers are informational; the caller already resolved the
/// target path through the workspace manager.
pub fn apply_unified_patch(original: &str, patch: &str) -> Result<String, String> {
    // Hunk headers from local models are routinely wrong about line numbers
    // and counts, while the context and removed lines are usually right. Each
    // hunk is therefore located by its old-side lines (exact first, then
    // ignoring surrounding whitespace), starting from the header's hint and
    // scanning forward from the previous hunk. Counts in the header are not
    // trusted; the located lines are.
    struct Hunk {
        hint: usize,
        lines: Vec<(char, String)>,
    }
    let mut hunks: Vec<Hunk> = Vec::new();
    for line in patch.lines() {
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("diff ") {
            continue;
        }
        if line.starts_with("@@") {
            let (start, _) = parse_hunk_header(line)?;
            hunks.push(Hunk {
                hint: start.saturating_sub(1),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = hunks.last_mut() else {
            return Err(format!("expected '@@' hunk header, got: {line}"));
        };
        let (kind, text) = if line.is_empty() {
            (' ', "")
        } else {
            let mut chars = line.chars();
            (chars.next().unwrap_or(' '), chars.as_str())
        };
        match kind {
            ' ' | '-' | '+' => hunk.lines.push((kind, text.to_string())),
            '\\' => {} // "\ No newline at end of file"
            _ => {
                return Err(format!(
                    "bad hunk line: {line:?} (must start with ' ', '-', '+')"
                ))
            }
        }
    }
    if hunks.is_empty() {
        return Err("patch contains no hunks".into());
    }
    let orig: Vec<&str> = original.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    for hunk in &hunks {
        let old: Vec<&str> = hunk
            .lines
            .iter()
            .filter(|(kind, _)| *kind != '+')
            .map(|(_, text)| text.as_str())
            .collect();
        let at = if old.is_empty() {
            // Pure insertion: the header position is all there is.
            hunk.hint.clamp(cursor, orig.len())
        } else {
            locate_lines(&orig, &old, cursor, hunk.hint).ok_or_else(|| {
                format!(
                    "the hunk's original lines were not found in the file (first expected line: {:?}). Re-read the file and use edit_file with old/new: old = the exact existing lines, new = their replacement.",
                    old[0]
                )
            })?
        };
        out.extend(orig[cursor..at].iter().map(|line| line.to_string()));
        let mut position = at;
        for (kind, text) in &hunk.lines {
            match kind {
                ' ' => {
                    // Keep the file's own line (whitespace may differ from the hunk).
                    out.push(orig.get(position).map(|l| l.to_string()).unwrap_or_else(|| text.clone()));
                    position += 1;
                }
                '-' => position += 1,
                _ => out.push(text.clone()),
            }
        }
        cursor = position.min(orig.len());
    }
    out.extend(orig[cursor..].iter().map(|line| line.to_string()));
    let mut result = out.join("\n");
    if original.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// Find `needle` (a run of lines) in `haystack` at or after `from`: exact
/// match nearest to `hint` first, then a whitespace-insensitive match.
fn locate_lines(haystack: &[&str], needle: &[&str], from: usize, hint: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len().saturating_sub(from) {
        return None;
    }
    let candidates = |equal: &dyn Fn(&str, &str) -> bool| -> Vec<usize> {
        (from..=haystack.len() - needle.len())
            .filter(|&start| {
                needle
                    .iter()
                    .enumerate()
                    .all(|(offset, line)| equal(haystack[start + offset], line))
            })
            .collect()
    };
    let nearest = |found: Vec<usize>| {
        found
            .into_iter()
            .min_by_key(|start| start.abs_diff(hint))
    };
    nearest(candidates(&|a, b| a == b))
        .or_else(|| nearest(candidates(&|a, b| a.trim() == b.trim())))
}

/// The file lines that best resemble the block the model tried to match:
/// the window whose lines share the most leading characters (after trimming)
/// with the requested lines. Shown verbatim so the model can copy them.
/// Where `needle`'s lines (compared without surrounding spaces) occur in
/// `haystack`, as 0-based starting lines.
fn line_block_positions(haystack: &[&str], needle: &[&str]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    (0..=haystack.len() - needle.len())
        .filter(|&start| {
            needle
                .iter()
                .enumerate()
                .all(|(offset, line)| haystack[start + offset].trim() == line.trim())
        })
        .collect()
}

/// Lines `from..to` (0-based, end exclusive) labelled as read_file labels them.
fn numbered_lines(lines: &[&str], from: usize, to: usize) -> String {
    lines[from.min(lines.len())..to.min(lines.len())]
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("{}: {line}", from + offset + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether an edit is already in the file: its new text is there and its old
/// text is not - or, for an insertion (new extends old), new is there as a
/// whole. Only for a new text long enough not to be found by chance.
fn edit_already_made(original: &str, old: &str, new: &str) -> bool {
    let text = original.replace("\r\n", "\n");
    let old = old.replace("\r\n", "\n");
    let new = new.replace("\r\n", "\n");
    let (old_block, new_block) = (old.trim_matches('\n'), new.trim_matches('\n'));
    if new_block.trim().chars().count() < 16 {
        return false;
    }
    let lines: Vec<&str> = text.lines().collect();
    let present = |block: &str| {
        text.contains(block) || !line_block_positions(&lines, &block.lines().collect::<Vec<_>>()).is_empty()
    };
    if !present(new_block) {
        return false;
    }
    !present(old_block) || (new_block.contains(old_block) && text.contains(new_block))
}

/// "lines 12-15" (1-based) for the part of a file an edit changed.
fn changed_lines(before: &str, after: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    if a == b {
        return "line endings only".to_string();
    }
    let first = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let same_tail = a[first..]
        .iter()
        .rev()
        .zip(b[first..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let last = b.len().saturating_sub(same_tail);
    match (first + 1, last) {
        (start, end) if end < start => format!("lines removed after line {}", first),
        (start, end) if start == end => format!("line {start}"),
        (start, end) => format!("lines {start}-{end}"),
    }
}

/// Why an edit's old text was not found, pointing at the line where it parts
/// from the file. The closest block alone was not enough: a model sent the
/// same 4,000-character old text four times with two stale places in it
/// (owner's 26B run, 2026-09-18).
fn not_found_message(haystack: &[&str], needle: &[&str]) -> String {
    let advice = if needle.len() > 20 {
        format!(
            "Your old text is {} lines long: send only the few lines you are changing, copied from the file as it is now - or, to change most of the file, write the whole file with write_file.",
            needle.len()
        )
    } else {
        "Copy those lines from the file as it is now into old (without the line-number labels), then give their replacement in new.".to_string()
    };
    let Some(start) = closest_start(haystack, needle) else {
        return format!(
            "old text was not found in the file, and no part of the file resembles it: the file may not hold what you remember. Read it again. {advice}"
        );
    };
    let differs = needle
        .iter()
        .enumerate()
        .find(|(offset, line)| haystack.get(start + offset).map_or(true, |file| file.trim() != line.trim()));
    let Some((offset, old_line)) = differs else {
        return format!("old text was not found in the file. {advice}");
    };
    let at = start + offset;
    let matched = if offset > 0 {
        format!(" It matches the file from line {} to line {at}, then differs at line {}:", start + 1, at + 1)
    } else {
        format!(" The closest place is line {}:", at + 1)
    };
    let file_line = haystack.get(at).map(|line| line.trim()).unwrap_or("(the file ends here)");
    format!(
        "old text was not found in the file.{matched}\n  the file has: {file_line}\n  old has:      {}\nThe file around line {}:\n{}\n{advice}",
        old_line.trim(),
        at + 1,
        numbered_lines(haystack, at.saturating_sub(2), at + 3)
    )
}

/// The start (0-based) where `needle` lines up best with `haystack`, by how
/// much of each line matches; None when nothing resembles it.
fn closest_start(haystack: &[&str], needle: &[&str]) -> Option<usize> {
    if haystack.is_empty() || needle.is_empty() {
        return None;
    }
    let similarity = |a: &str, b: &str| {
        let (a, b) = (a.trim(), b.trim());
        let common = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
        if a.is_empty() || b.is_empty() {
            0
        } else {
            common * 100 / a.len().max(b.len())
        }
    };
    let window = needle.len().min(haystack.len());
    let (start, score) = (0..=haystack.len() - window)
        .map(|start| {
            let score: usize = needle
                .iter()
                .take(window)
                .enumerate()
                .map(|(offset, line)| similarity(haystack[start + offset], line))
                .sum();
            (start, score)
        })
        .max_by_key(|(start, score)| (*score, usize::MAX - *start))?;
    (score > 0).then_some(start)
}

fn closest_region(haystack: &[&str], needle: &[&str]) -> Option<String> {
    if haystack.is_empty() || needle.is_empty() {
        return None;
    }
    let similarity = |a: &str, b: &str| {
        let (a, b) = (a.trim(), b.trim());
        let common = a
            .chars()
            .zip(b.chars())
            .take_while(|(x, y)| x == y)
            .count();
        // Reward matching content, not matching emptiness.
        if a.is_empty() || b.is_empty() {
            0
        } else {
            common * 100 / a.len().max(b.len())
        }
    };
    let window = needle.len().min(haystack.len());
    let (best_start, best_score) = (0..=haystack.len() - window)
        .map(|start| {
            let score: usize = needle
                .iter()
                .take(window)
                .enumerate()
                .map(|(offset, line)| similarity(haystack[start + offset], line))
                .sum();
            (start, score)
        })
        .max_by_key(|(start, score)| (*score, usize::MAX - *start))?;
    if best_score == 0 {
        return None;
    }
    let from = best_start.saturating_sub(1);
    let to = (best_start + window + 1).min(haystack.len());
    Some(
        haystack[from..to]
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Exact-text replacement with the fallbacks small models need: CRLF/LF
/// tolerance, a whitespace-insensitive line match when indentation was
/// reproduced imperfectly, and stripping of `N: ` line-number labels copied
/// from read_file output. The match must be unique unless `replace_all`.
pub fn apply_replacement(
    original: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<String, String> {
    if old.is_empty() {
        return Err("old text is empty: give the exact existing lines to replace".into());
    }
    let crlf = original.contains("\r\n");
    let text = original.replace("\r\n", "\n");
    let old_lf = old.replace("\r\n", "\n");
    let new_lf = new.replace("\r\n", "\n");
    let finish = |updated: String| {
        if crlf {
            updated.replace('\n', "\r\n")
        } else {
            updated
        }
    };
    let count = text.matches(old_lf.as_str()).count();
    if count == 1 || (count > 1 && replace_all) {
        return Ok(finish(text.replace(old_lf.as_str(), &new_lf)));
    }
    if count > 1 {
        return Err(format!(
            "old text occurs {count} times; include more surrounding lines so it is unique, or set replace_all: true"
        ));
    }
    // Strip "12: " style labels a model may have copied from read_file output.
    let unlabelled: Vec<String> = old_lf
        .lines()
        .map(|line| {
            let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && line[digits..].starts_with(": ") {
                line[digits + 2..].to_string()
            } else if digits > 0 && line[digits..] == *":" {
                String::new()
            } else {
                line.to_string()
            }
        })
        .collect();
    let stripped = unlabelled.join("\n");
    if unlabelled.len() == old_lf.lines().count() && stripped != old_lf {
        let count = text.matches(stripped.as_str()).count();
        if count == 1 {
            return Ok(finish(text.replace(stripped.as_str(), &new_lf)));
        }
    }
    // Whitespace-insensitive line match: locate the block, replace it whole.
    let haystack: Vec<&str> = text.lines().collect();
    let needle: Vec<&str> = stripped.lines().collect();
    let starts: Vec<usize> = if needle.is_empty() || needle.len() > haystack.len() {
        Vec::new()
    } else {
        (0..=haystack.len() - needle.len())
            .filter(|&start| {
                needle
                    .iter()
                    .enumerate()
                    .all(|(offset, line)| haystack[start + offset].trim() == line.trim())
            })
            .collect()
    };
    match starts.len() {
        1 => {
            let start = starts[0];
            let mut out: Vec<String> = haystack[..start].iter().map(|l| l.to_string()).collect();
            out.extend(new_lf.lines().map(|l| l.to_string()));
            out.extend(haystack[start + needle.len()..].iter().map(|l| l.to_string()));
            let mut updated = out.join("\n");
            if text.ends_with('\n') {
                updated.push('\n');
            }
            Ok(finish(updated))
        }
        0 => Err(not_found_message(&haystack, &needle)),
        n => Err(format!(
            "old text matches {n} places when ignoring indentation; include more surrounding lines so it is unique"
        )),
    }
}

fn parse_hunk_header(line: &str) -> Result<(usize, usize), String> {
    // Format: @@ -old_start[,old_len] +new_start[,new_len] @@ [section]
    let inner = line
        .strip_prefix("@@")
        .and_then(|s| s.split("@@").next())
        .ok_or_else(|| format!("bad hunk header: {line}"))?;
    let mut parts = inner.split_whitespace();
    let old = parts
        .next()
        .ok_or_else(|| format!("bad hunk header: {line}"))?;
    let old = old
        .strip_prefix('-')
        .ok_or_else(|| format!("bad hunk header: {line}"))?;
    let (start, len) = match old.split_once(',') {
        Some((a, b)) => (
            a.parse::<usize>()
                .map_err(|_| format!("bad hunk header: {line}"))?,
            b.parse::<usize>()
                .map_err(|_| format!("bad hunk header: {line}"))?,
        ),
        None => (
            old.parse::<usize>()
                .map_err(|_| format!("bad hunk header: {line}"))?,
            1,
        ),
    };
    Ok((start, len))
}

/// Recursive regex search (§24). Large source files are scanned line by line.
/// Returns `path:line: match` lines, capped — deterministic and terse for
/// small local models (§95: relevant sections, not dumps).
fn search_dir(dir: &std::path::Path, root: &std::path::Path, re: &regex::Regex) -> String {
    // The folder searched is a resolved path (on Windows it carries the
    // long-path prefix); the root it is named from must be resolved the same
    // way, or nothing is stripped and every hit shows its full path.
    let resolved_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root = resolved_root.as_path();
    let mut hits: Vec<String> = vec![];
    if dir.is_file() {
        search_file(dir, root, re, &mut hits);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if hits.len() >= MAX_SEARCH_MATCHES {
            break;
        }
        let entries = match std::fs::read_dir(&d) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            if hits.len() >= MAX_SEARCH_MATCHES {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "node_modules" | "target" | "build" | "dist" | "vendor" | "__pycache__"
                )
            {
                continue;
            }
            let ft = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                search_file(&path, root, re, &mut hits);
            }
        }
    }
    if hits.is_empty() {
        "(no matches)".into()
    } else {
        let mut out = hits.join("\n");
        if hits.len() >= MAX_SEARCH_MATCHES {
            out.push_str(&format!("\n(capped at {MAX_SEARCH_MATCHES} matches)"));
        }
        out
    }
}

fn search_file(
    path: &std::path::Path,
    root: &std::path::Path,
    re: &regex::Regex,
    hits: &mut Vec<String>,
) {
    use std::io::BufRead;
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let mut reader = std::io::BufReader::new(file);
    // Avoid interpreting binary/model assets as code, without a source-size cap.
    match reader.fill_buf() {
        Ok(bytes) if !bytes.contains(&0) => {}
        _ => return,
    }
    for (n, line) in reader.lines().enumerate() {
        if hits.len() >= MAX_SEARCH_MATCHES {
            break;
        }
        let Ok(line) = line else {
            break;
        };
        if line.contains('\0') {
            break;
        }
        if re.is_match(&line) {
            // Named from the project root, with forward slashes: named from
            // the folder searched, `src/App.tsx` came back as `App.tsx`, and
            // the model's next read_file of it failed.
            let shown = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            hits.push(format!(
                "{shown}:{}: {}",
                n + 1,
                line.chars().take(200).collect::<String>()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ws() -> WorkspaceManager {
        // Unique per call: parallel tests must never share a scratch dir.
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("companion-tool-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "hello").unwrap();
        WorkspaceManager::new(dir)
    }

    #[test]
    fn every_listed_required_argument_is_refused_when_missing() {
        let w = ws();
        for tool in registry().into_iter().filter(|tool| !runs_in_the_agent(tool.name)) {
            for (missing, _) in required_args(tool.name) {
                let args: serde_json::Map<String, serde_json::Value> = required_args(tool.name)
                    .iter()
                    .filter(|(name, _)| name != missing)
                    .map(|(name, _)| (name.to_string(), serde_json::json!("a.txt")))
                    .collect();
                let request = ToolRequest { name: tool.name.into(), args: serde_json::Value::Object(args), approved: true };
                let error = execute(&request, &w, true).unwrap_err().to_string();
                assert!(error.contains("requires"), "{} without {missing}: {error}", tool.name);
            }
        }
    }

    #[test]
    fn edits_inside_git_folders_are_recognised_on_any_separator() {
        for path in [".git/config", "project/.git/hooks/pre-commit", ".GIT\\config", "a\\.git\\info"] {
            assert!(touches_git_internals("write_file", &serde_json::json!({"path": path})), "{path}");
        }
        assert!(!touches_git_internals("write_file", &serde_json::json!({"path": ".gitignore"})));
        assert!(!touches_git_internals("write_file", &serde_json::json!({"path": "src/git/config.rs"})));
        assert!(!touches_git_internals("read_file", &serde_json::json!({"path": ".git/config"})), "reads are not edits");
    }

    #[test]
    fn a_missing_path_names_the_projects_files_of_that_name() {
        let w = ws();
        std::fs::create_dir_all(w.root().join("project")).unwrap();
        std::fs::write(w.root().join("project").join("orders.py"), "x = 1\n").unwrap();
        let read = |path: &str| {
            let request = ToolRequest { name: "read_file".into(), args: serde_json::json!({"path": path}), approved: false };
            execute(&request, &w, false).unwrap_err().to_string()
        };
        let message = read("orders.py");
        assert!(message.contains("no such path 'orders.py'") && message.contains("project/orders.py"), "{message}");
        let none = read("missing.py");
        assert!(none.contains("list_directory shows what exists"), "{none}");
        let listed = execute(&ToolRequest { name: "list_directory".into(), args: serde_json::json!({"path": "src"}), approved: false }, &w, false)
            .unwrap_err()
            .to_string();
        assert!(listed.contains("no such path 'src'"), "{listed}");
    }

    /// The whole path a file body travels: the model's reply, parsed as an
    /// action, executed, read back from disk. The body is the kind that broke
    /// a real run when it went through JSON escaping.
    #[test]
    fn a_raw_file_body_reaches_disk_byte_for_byte_and_appends_in_parts() {
        let w = ws();
        let first = r#"(() => {
  "use strict";
  const re = /\d+\.\d+/;
  const msg = "it's \"quoted\"";
  const tpl = `line ${a}`;
"#;
        let second = r#"  const path = "C:\Users\name";
})();
"#;
        let reply = format!(
            concat!(
                "Writing the script.\n```tool\n",
                r#"{{"name":"write_file","args":{{"path":"app.js"}}}}"#,
                "\n<<<CONTENT\n{first}CONTENT>>>\n```\n"
            ),
            first = first
        );
        let call = crate::agent_runner::parse_tool_block(&reply).expect("parsed");
        assert_eq!(call.name, "write_file");
        let request = ToolRequest {
            name: call.name.clone(),
            args: call.args.clone(),
            approved: true,
        };
        let result = execute(&request, &w, true).unwrap();
        assert!(result.output.contains(WRITE_VERIFIED), "{}", result.output);

        // The second part arrives as its own action, as a small window forces.
        let appended = format!(
            concat!(
                "```tool\n",
                r#"{{"name":"append_file","args":{{"path":"app.js"}}}}"#,
                "\n<<<CONTENT\n{second}CONTENT>>>\n```\n"
            ),
            second = second
        );
        let call = crate::agent_runner::parse_tool_block(&appended).expect("parsed append");
        let request = ToolRequest {
            name: call.name.clone(),
            args: call.args.clone(),
            approved: true,
        };
        execute(&request, &w, true).unwrap();

        let on_disk = std::fs::read_to_string(w.root().join("app.js")).unwrap();
        assert_eq!(on_disk, format!("{first}{second}"));
        // The characters that used to be lost survived: an apostrophe, an
        // escaped quote, a regex backslash, a backtick and a Windows path.
        assert!(on_disk.contains(r#"const re = /\d+\.\d+/;"#));
        assert!(on_disk.contains(r#"const msg = "it's \"quoted\"";"#));
        assert!(on_disk.contains(r#""C:\Users\name""#));
    }

    #[test]
    fn registry_marks_command_dangerous() {
        let reg = registry();
        let cmd = reg.iter().find(|t| t.name == "execute_command").unwrap();
        assert_eq!(cmd.risk, RiskLevel::Dangerous);
    }

    #[test]
    fn search_large_source_and_then_read_deep_match() {
        let w = ws();
        let text = format!(
            "{}deep_unique_marker\n",
            format!("{}\n", "x".repeat(90)).repeat(17_000)
        );
        std::fs::write(w.root().join("large.cpp"), text).unwrap();
        for path in [".", "large.cpp"] {
            let request = ToolRequest {
                name: "search_text".into(),
                args: serde_json::json!({"path":path,"query":"deep_unique_marker"}),
                approved: false,
            };
            let result = execute(&request, &w, false).unwrap();
            assert!(result
                .output
                .contains("large.cpp:17001: deep_unique_marker"));
        }
        let request = ToolRequest {
            name: "read_file".into(),
            args: serde_json::json!({"path":"large.cpp","start_line":17001}),
            approved: false,
        };
        assert!(execute(&request, &w, false)
            .unwrap()
            .output
            .contains("17001: deep_unique_marker"));
    }

    #[test]
    fn read_file_inside_workspace_works() {
        let w = ws();
        let req = ToolRequest {
            name: "read_file".into(),
            args: serde_json::json!({"path": "a.txt"}),
            approved: false,
        };
        let r = execute(&req, &w, false).unwrap();
        assert!(r.ok && r.output.contains("hello"));
    }

    #[test]
    fn read_file_blocks_traversal() {
        let w = WorkspaceManager::new(PathBuf::from("/tmp/ws-root-test"));
        let req = ToolRequest {
            name: "read_file".into(),
            args: serde_json::json!({"path": "../../etc/passwd"}),
            approved: false,
        };
        assert!(matches!(
            execute(&req, &w, false).unwrap_err(),
            ToolError::Workspace(_)
        ));
    }

    #[test]
    fn write_requires_approval() {
        let w = ws();
        let req = ToolRequest {
            name: "write_file".into(),
            args: serde_json::json!({"path": "b.txt", "content": "x"}),
            approved: false,
        };
        assert!(matches!(
            execute(&req, &w, false).unwrap_err(),
            ToolError::PermissionRequired { .. }
        ));
    }

    #[test]
    fn unknown_tool_errors() {
        let w = ws();
        let req = ToolRequest {
            name: "nuke".into(),
            args: serde_json::json!({}),
            approved: true,
        };
        assert!(matches!(
            execute(&req, &w, true).unwrap_err(),
            ToolError::UnknownTool(_)
        ));
    }

    fn wsfresh(name: &str) -> WorkspaceManager {
        let dir = std::env::temp_dir().join(format!("companion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        WorkspaceManager::new(dir)
    }

    #[test]
    fn write_creates_nested_and_roundtrips() {
        let w = wsfresh("write");
        let req = ToolRequest {
            name: "write_file".into(),
            args: serde_json::json!({"path": "sub/b.txt", "content": "hello"}),
            approved: true,
        };
        let r = execute(&req, &w, false).unwrap();
        assert!(r.ok);
        let back = execute(
            &ToolRequest {
                name: "read_file".into(),
                args: serde_json::json!({"path": "sub/b.txt"}),
                approved: false,
            },
            &w,
            false,
        )
        .unwrap();
        assert!(back.output.contains("1: hello\n"));
        assert!(back.output.contains("EOF: no further content"));
        assert_eq!(
            std::fs::read_to_string(w.root().join("sub/b.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn lines_are_replaced_by_number_and_a_wrong_number_is_refused() {
        let dir = std::env::temp_dir().join(format!("companion-lines-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let w = crate::workspace::WorkspaceManager::new(dir.clone());
        let original = "one\ntwo\nthree\nfour\nfive\n";
        std::fs::write(dir.join("a.txt"), original).unwrap();
        let call = |args: serde_json::Value| ToolRequest { name: "replace_lines".into(), args, approved: true };

        // Three lines become one, and the answer says where the numbers moved.
        let result = execute(&call(serde_json::json!({"path": "a.txt", "from": 2, "to": 4, "text": "TWO-FOUR"})), &w, true).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\nTWO-FOUR\nfive\n");
        assert!(result.output.contains("replaced lines 2-4"), "{}", result.output);
        assert!(result.output.contains("3 line(s) -> 1 line(s)"), "{}", result.output);
        assert!(result.output.contains("it is now 3 lines"), "{}", result.output);
        assert!(result.output.contains("read it again"), "{}", result.output);

        // The anchor catches text that is not there, shows the real line,
        // and changes nothing.
        let stale = execute(&call(serde_json::json!({"path": "a.txt", "from": 2, "to": 2, "text": "x", "expect": "two"})), &w, true).unwrap_err();
        assert!(format!("{stale}").contains("2: TWO-FOUR"), "{stale}");
        assert!(!format!("{stale}").contains("have moved"), "{stale}");
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\nTWO-FOUR\nfive\n");

        // With the right anchor it goes through.
        execute(&call(serde_json::json!({"path": "a.txt", "from": 2, "to": 2, "text": "second", "expect": "TWO-FOUR"})), &w, true).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\nsecond\nfive\n");

        // Empty text deletes those lines.
        execute(&call(serde_json::json!({"path": "a.txt", "from": 3, "to": 3, "text": ""})), &w, true).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "one\nsecond\n");

        // A number past the end says how many lines there are, rather than appending.
        let past = execute(&call(serde_json::json!({"path": "a.txt", "from": 9, "to": 9, "text": "x"})), &w, true).unwrap_err();
        assert!(format!("{past}").contains("has 2 lines"), "{past}");
        assert!(execute(&call(serde_json::json!({"path": "a.txt", "from": 0, "to": 1, "text": "x"})), &w, true).is_err(), "lines start at 1");
        assert!(execute(&call(serde_json::json!({"path": "a.txt", "from": 2, "to": 1, "text": "x"})), &w, true).is_err(), "to before from");
        assert!(execute(&call(serde_json::json!({"path": "gone.txt", "from": 1, "to": 1, "text": "x"})), &w, true).is_err());

        // A range that runs past the end takes what is there.
        std::fs::write(dir.join("b.txt"), "a\nb\nc\n").unwrap();
        let clamped = execute(&call(serde_json::json!({"path": "b.txt", "from": 2, "to": 99, "text": "B"})), &w, true).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "a\nB\n");
        assert!(clamped.output.contains("replaced lines 2-3"), "{}", clamped.output);

        // The host's own context note is not file text here either.
        let note = "[Released: 900 characters of file text were released from working context.]";
        assert!(execute(&call(serde_json::json!({"path": "b.txt", "from": 1, "to": 1, "text": note})), &w, true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_outline_answers_where_things_are_without_the_whole_file() {
        let dir = std::env::temp_dir().join(format!("companion-outline-tool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let w = crate::workspace::WorkspaceManager::new(dir.clone());
        std::fs::write(dir.join("app.py"), "import os\n\n\ndef load(path):\n    return path\n\n\nclass Store:\n    pass\n").unwrap();
        let file = execute(&ToolRequest { name: "outline".into(), args: serde_json::json!({"path": "app.py"}), approved: false }, &w, false).unwrap();
        assert!(file.output.contains("4: def load(path):"), "{}", file.output);
        assert!(file.output.contains("8: class Store:"), "{}", file.output);
        let folder = execute(&ToolRequest { name: "outline".into(), args: serde_json::json!({"path": "."}), approved: false }, &w, false).unwrap();
        assert!(folder.output.contains("app.py (9 lines"), "{}", folder.output);
        assert!(execute(&ToolRequest { name: "outline".into(), args: serde_json::json!({"path": "nope.py"}), approved: false }, &w, false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hosts_own_context_note_is_never_written_into_a_file() {
        let dir = std::env::temp_dir().join(format!("companion-note-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ws = crate::workspace::WorkspaceManager::new(dir.clone());
        std::fs::write(dir.join("app.js"), "const real = 1;\n").unwrap();
        let note = "[Released: 1403 characters of file text were released from working context to stay within the model's window. The file on disk has them.]";
        for tool in ["write_file", "append_file"] {
            let request = ToolRequest { name: tool.into(), args: serde_json::json!({"path": "app.js", "content": note}), approved: true };
            let error = execute(&request, &ws, true).unwrap_err();
            assert!(format!("{error}").contains("context note"), "{tool}: {error}");
        }
        let edit = ToolRequest {
            name: "edit_file".into(),
            args: serde_json::json!({"path": "app.js", "old": "const real = 1;", "new": note}),
            approved: true,
        };
        assert!(execute(&edit, &ws, true).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("app.js")).unwrap(), "const real = 1;\n", "the file keeps its text");
        // Text that merely mentions the note is still file text.
        let honest = ToolRequest {
            name: "write_file".into(),
            args: serde_json::json!({"path": "notes.md", "content": "The host writes [Released: ...] when it frees context."}),
            approved: true,
        };
        assert!(execute(&honest, &ws, true).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn patch_applies_and_rejects_stale_context() {
        let original = "line1\nline2\nline3\n";
        let patch = "@@ -1,3 +1,3 @@\n line1\n-line2\n+LINE2\n line3\n";
        assert_eq!(
            apply_unified_patch(original, patch).unwrap(),
            "line1\nLINE2\nline3\n"
        );
        let stale = "@@ -1,3 +1,3 @@\n line1\n-CHANGED\n+X\n line3\n";
        let err = apply_unified_patch(original, stale).unwrap_err();
        assert!(err.contains("not found"), "{err}");
        assert!(apply_unified_patch(original, "no hunks here").is_err());
    }

    #[test]
    fn replacement_edit_tolerates_labels_indentation_and_line_endings() {
        let original = "function subtract(a, b) {\n  return a + b; // BUG\n}\n\nfunction add(a, b) {\n  return a + b;\n}\n";
        // Unique exact match.
        let fixed = apply_replacement(original, "  return a + b; // BUG", "  return a - b;", false).unwrap();
        assert!(fixed.contains("return a - b;") && !fixed.contains("BUG"));
        // Ambiguous without replace_all.
        let err = apply_replacement(original, "  return a + b;", "x", false).unwrap_err();
        assert!(err.contains("2 times"), "{err}");
        assert_eq!(apply_replacement(original, "  return a + b;", "  return 0;", true).unwrap().matches("return 0;").count(), 2);
        // Line-number labels copied from read_file output are stripped.
        let labelled = apply_replacement(original, "1: function subtract(a, b) {\n2:   return a + b; // BUG", "function subtract(a, b) {\n  return a - b;", false).unwrap();
        assert!(labelled.starts_with("function subtract(a, b) {\n  return a - b;\n}"));
        // Indentation reproduced imperfectly still locates the block.
        let loose = apply_replacement(original, "return a + b; // BUG", "  return a - b;", false).unwrap();
        assert!(loose.contains("  return a - b;\n}"));
        // CRLF files keep CRLF.
        let crlf = original.replace('\n', "\r\n");
        let fixed_crlf = apply_replacement(&crlf, "  return a + b; // BUG", "  return a - b;", false).unwrap();
        assert!(fixed_crlf.contains("return a - b;\r\n}"));
        assert!(!fixed_crlf.contains("\n\n") || fixed_crlf.contains("\r\n\r\n"));
        assert!(apply_replacement(original, "nothing like this", "x", false).unwrap_err().contains("not found"));
        assert!(apply_replacement(original, "", "x", false).is_err());
        // A near miss (the model dropped the trailing comment) shows the real
        // lines so the next attempt can copy them.
        let err = apply_replacement(original, "function subtract(a, b) {\n  return a + b;\n}", "x", false).unwrap_err();
        assert!(err.contains("then differs at line 2"), "{err}");
        assert!(err.contains("return a + b; // BUG"), "{err}");
    }

    #[test]
    fn unified_patch_locates_hunks_by_content_not_by_wrong_headers() {
        let original = "// header\nfunction add(a, b) {\n  return a + b;\n}\n\nfunction subtract(a, b) {\n  return a + b; // BUG\n}\n";
        // Wrong start line and counts (what an 8B model produced), right lines.
        let patch = "--- a/calc.js\n+++ b/calc.js\n@@ -1,2 +1,2 @@\n function subtract(a, b) {\n-  return a + b; // BUG\n+  return a - b;\n }\n";
        let fixed = apply_unified_patch(original, patch).unwrap();
        assert!(fixed.contains("function subtract(a, b) {\n  return a - b;\n}"));
        assert!(fixed.contains("function add(a, b) {\n  return a + b;\n}"), "the other function is untouched");
        // Lines that do not exist anywhere are still rejected.
        let bogus = "@@ -6,2 +6,2 @@\n-  return a - b; // Fixed\n+  return a * b;\n";
        assert!(apply_unified_patch(original, bogus).unwrap_err().contains("not found"));
    }

    #[test]
    fn edit_tool_end_to_end() {
        let w = wsfresh("edit");
        std::fs::write(w.root().join("c.txt"), "a\nb\nc\n").unwrap();
        let req = ToolRequest {
            name: "edit_file".into(),
            args: serde_json::json!({"path": "c.txt", "patch": "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n"}),
            approved: true,
        };
        assert!(execute(&req, &w, false).unwrap().ok);
        assert_eq!(
            std::fs::read_to_string(w.root().join("c.txt")).unwrap(),
            "a\nB\nc\n"
        );
    }

    #[test]
    fn search_finds_and_caps() {
        let w = wsfresh("search");
        std::fs::write(
            w.root().join("x.rs"),
            "fn temporal() {}\n// temporal accumulation\n",
        )
        .unwrap();
        std::fs::create_dir_all(w.root().join("sub")).unwrap();
        std::fs::write(w.root().join("sub").join("y.txt"), "nothing here\n").unwrap();
        let req = ToolRequest {
            name: "search_text".into(),
            args: serde_json::json!({"query": "temporal"}),
            approved: false,
        };
        let r = execute(&req, &w, false).unwrap();
        assert!(r.output.contains("x.rs:1"), "{}", r.output);
        assert!(r.output.contains("x.rs:2"), "{}", r.output);
        // Not a valid expression: searched for as typed, and said so.
        let bad = ToolRequest {
            name: "search_text".into(),
            args: serde_json::json!({"query": "(unclosed"}),
            approved: false,
        };
        let plain = execute(&bad, &w, false).unwrap().output;
        assert!(plain.contains("plain text") && plain.contains("(no matches)"), "{plain}");
    }

    #[test]
    fn writes_are_read_back_by_the_host_and_missing_paths_are_not_opened() {
        let w = wsfresh("verify");
        let req = ToolRequest {
            name: "write_file".into(),
            args: serde_json::json!({"path": "site/index.html", "content": "<h1>Hi</h1>\n<p>Energy</p>\n"}),
            approved: true,
        };
        let result = execute(&req, &w, false).unwrap();
        assert!(result.output.contains(WRITE_VERIFIED), "{}", result.output);
        assert!(result.output.contains("(2 lines)"), "{}", result.output);
        assert_eq!(std::fs::read_to_string(w.root().join("site/index.html")).unwrap(), "<h1>Hi</h1>\n<p>Energy</p>\n");
        // open_path never hands the OS a path that does not exist (Explorer
        // would open an unrelated window and the call would look successful).
        let open = ToolRequest {
            name: "open_path".into(),
            args: serde_json::json!({"path": "energy-drink.txt"}),
            approved: true,
        };
        let error = execute(&open, &w, false).unwrap_err().to_string();
        assert!(error.contains("nothing exists at energy-drink.txt"), "{error}");
        assert!(error.contains("Artifacts panel"), "{error}");
    }

    #[test]
    fn delete_needs_approval_and_removes() {
        let w = wsfresh("del");
        std::fs::write(w.root().join("d.txt"), "bye").unwrap();
        let req = ToolRequest {
            name: "delete_file".into(),
            args: serde_json::json!({"path": "d.txt"}),
            approved: false,
        };
        assert!(matches!(
            execute(&req, &w, false).unwrap_err(),
            ToolError::PermissionRequired { .. }
        ));
        assert_eq!(risk_of("delete_file"), RiskLevel::Dangerous);
        let req = ToolRequest {
            name: "delete_file".into(),
            args: serde_json::json!({"path": "d.txt"}),
            approved: true,
        };
        assert!(execute(&req, &w, false).unwrap().ok);
        assert!(!w.root().join("d.txt").exists());
    }

    #[test]
    fn chat_safe_set_is_read_only() {
        for t in [
            "list_directory",
            "read_file",
            "search_text",
            "system_info",
            "list_processes",
        ] {
            assert!(chat_safe(t), "{t}");
        }
        for t in [
            "write_file",
            "edit_file",
            "delete_file",
            "execute_command",
            "git_commit",
            "open_path",
        ] {
            assert!(!chat_safe(t), "{t}");
        }
        // Documents never touch the project: they render into the app's own
        // artifacts folder, so chat may produce them without approval.
        assert!(chat_safe("create_document"));
    }

    #[test]
    fn git_commit_needs_approval_and_clean_message() {
        let w = wsfresh("gitc");
        let denied = ToolRequest {
            name: "git_commit".into(),
            args: serde_json::json!({"message": "ok"}),
            approved: false,
        };
        assert!(matches!(
            execute(&denied, &w, false).unwrap_err(),
            ToolError::PermissionRequired { .. }
        ));
        let evil = ToolRequest {
            name: "git_commit".into(),
            args: serde_json::json!({"message": "x\"; rm -rf /"}),
            approved: true,
        };
        assert!(matches!(
            execute(&evil, &w, true).unwrap_err(),
            ToolError::InvalidArgs(_)
        ));
        assert_eq!(risk_of("git_commit"), RiskLevel::Moderate);
    }

    #[test]
    fn list_processes_returns_rows() {
        let w = wsfresh("procs");
        let req = ToolRequest {
            name: "list_processes".into(),
            args: serde_json::json!({}),
            approved: false,
        };
        let r = execute(&req, &w, false).unwrap();
        assert!(
            r.output.contains("pid "),
            "{}",
            r.output.chars().take(200).collect::<String>()
        );
    }
}


/// The whole tool set, driven as the agent drives it: success and failure,
/// and whether each failure says what to do next (owner, 2026-09-18: "all
/// tools need thorough testing"; edit_file had failed 41 of its 72 calls).
#[cfg(test)]
mod tool_checks {
    use super::*;
    use serde_json::json;

    fn project(name: &str) -> WorkspaceManager {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("companion-tool-checks-{}-{n}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        WorkspaceManager::new(dir)
    }

    fn run(w: &WorkspaceManager, name: &str, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        execute(&ToolRequest { name: name.into(), args, approved: true }, w, true)
    }

    fn put(w: &WorkspaceManager, rel: &str, text: &str) {
        let path = w.root().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn get(w: &WorkspaceManager, rel: &str) -> String {
        std::fs::read_to_string(w.root().join(rel)).unwrap()
    }

    #[test]
    fn a_listing_marks_folders_and_says_what_a_file_an_empty_folder_or_a_wrong_path_is() {
        let w = project("site");
        put(&w, "a.txt", "hello");
        put(&w, "src/main.rs", "fn main() {}\n");
        std::fs::create_dir_all(w.root().join("src/empty")).unwrap();
        assert_eq!(run(&w, "list_directory", json!({"path": "."})).unwrap().output, "a.txt\nsrc/");
        assert_eq!(run(&w, "list_directory", json!({})).unwrap().output, "a.txt\nsrc/", "the root by default");
        assert_eq!(run(&w, "list_directory", json!({"path": "src"})).unwrap().output, "empty/\nmain.rs");
        assert!(run(&w, "list_directory", json!({"path": "src/empty"})).unwrap().output.contains("empty folder"));
        let file = run(&w, "list_directory", json!({"path": "src/main.rs"})).unwrap_err().to_string();
        assert!(file.contains("is a file") && file.contains("read_file"), "{file}");
        let missing = run(&w, "list_directory", json!({"path": "empty"})).unwrap_err().to_string();
        assert!(missing.contains("src/empty"), "the folder of that name is named: {missing}");
        assert!(run(&w, "list_directory", json!({"path": ".."})).is_err(), "nothing above the project");
    }

    #[test]
    fn reading_handles_line_endings_ranges_folders_binary_files_and_wrong_paths() {
        let w = project("site");
        put(&w, "notes/plan.md", "line one\r\nline two\r\nline three\r\n");
        std::fs::write(w.root().join("notes/logo.png"), [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0xff, 0xfe, 0x00]).unwrap();
        let whole = run(&w, "read_file", json!({"path": "notes/plan.md"})).unwrap().output;
        assert!(whole.contains("1: line one") && whole.contains("3: line three") && !whole.contains('\r'), "{whole}");
        let part = run(&w, "read_file", json!({"path": "notes/plan.md", "start_line": 2, "end_line": 2})).unwrap().output;
        assert!(part.contains("2: line two") && !part.contains("line one") && !part.contains("line three"), "{part}");
        let folder = run(&w, "read_file", json!({"path": "notes"})).unwrap_err().to_string();
        assert!(folder.contains("is a folder") && folder.contains("list_directory"), "{folder}");
        let binary = run(&w, "read_file", json!({"path": "notes/logo.png"})).unwrap_err().to_string();
        assert!(binary.contains("not a text file"), "{binary}");
        let missing = run(&w, "read_file", json!({"path": "plan.md"})).unwrap_err().to_string();
        assert!(missing.contains("notes/plan.md"), "{missing}");
        let backwards = run(&w, "read_file", json!({"path": "notes/plan.md", "start_line": 3, "end_line": 1})).unwrap_err().to_string();
        assert!(backwards.contains("end_line"), "{backwards}");
    }

    #[test]
    fn a_write_makes_its_folders_keeps_every_character_and_is_checked_on_disk() {
        let w = project("site");
        let text = "tab\there \"quotes\" 'single' back\\slash\r\nnon-ascii: São Paulo 25°C ✓\n{\"json\": [1, 2]}\n";
        let written = run(&w, "write_file", json!({"path": "deep/er/file.txt", "content": text})).unwrap();
        assert!(written.output.contains(WRITE_VERIFIED), "{}", written.output);
        assert_eq!(std::fs::read(w.root().join("deep/er/file.txt")).unwrap(), text.as_bytes());
        // Overwrites, and writes an empty file.
        run(&w, "write_file", json!({"path": "deep/er/file.txt", "content": "second"})).unwrap();
        assert_eq!(get(&w, "deep/er/file.txt"), "second");
        run(&w, "write_file", json!({"path": "empty.txt", "content": ""})).unwrap();
        assert_eq!(get(&w, "empty.txt"), "");
        assert!(run(&w, "write_file", json!({"path": "../escape.txt", "content": "x"})).is_err());
        let note = run(&w, "write_file", json!({"path": "x.js", "content": "[Released: 900 characters were released]"})).unwrap_err().to_string();
        assert!(note.contains("context note"), "{note}");
        assert!(!w.root().join("x.js").exists());
    }

    #[test]
    fn appending_builds_a_file_in_parts() {
        let w = project("site");
        let first = run(&w, "append_file", json!({"path": "log/out.txt", "content": "part one\n"})).unwrap().output;
        assert!(first.contains("it is now 9 bytes"), "{first}");
        run(&w, "append_file", json!({"path": "log/out.txt", "content": "part two\n"})).unwrap();
        assert_eq!(get(&w, "log/out.txt"), "part one\npart two\n");
    }

    #[test]
    fn an_edit_that_changes_nothing_or_was_made_already_says_so() {
        let w = project("site");
        put(&w, "app.js", "const a = 1;\nconst b = 2;\n");
        let same = run(&w, "edit_file", json!({"path": "app.js", "old": "const a = 1;", "new": "const a = 1;"})).unwrap_err().to_string();
        assert!(same.contains("same text") && same.contains("nothing"), "{same}");
        let edit = json!({"path": "app.js", "old": "const b = 2;", "new": "const b = 3; // tuned value"});
        let first = run(&w, "edit_file", edit.clone()).unwrap().output;
        assert!(first.contains("line 2") && first.contains("read back from disk"), "{first}");
        // The same edit again: made already, not "not found".
        let again = run(&w, "edit_file", edit).unwrap().output;
        assert!(again.contains("already holds the new text"), "{again}");
        assert_eq!(get(&w, "app.js"), "const a = 1;\nconst b = 3; // tuned value\n");
        // An insertion repeated is not inserted twice.
        let insert = json!({"path": "app.js", "old": "const a = 1;", "new": "const a = 1;\nconst inserted = true; // once"});
        run(&w, "edit_file", insert.clone()).unwrap();
        let twice = run(&w, "edit_file", insert).unwrap().output;
        assert!(twice.contains("already holds"), "{twice}");
        assert_eq!(get(&w, "app.js").matches("inserted").count(), 1);
        // A short new text is not taken as made already by chance.
        put(&w, "short.js", "x();\ny();\n");
        let short = run(&w, "edit_file", json!({"path": "short.js", "old": "z();", "new": "y();"})).unwrap_err().to_string();
        assert!(short.contains("not found"), "{short}");
    }

    #[test]
    fn an_edit_from_a_stale_copy_is_told_where_the_file_differs() {
        let w = project("site");
        let file: String = (1..=40).map(|n| format!("line {n} of the component\n")).collect();
        put(&w, "App.tsx", &file);
        // The model's copy: the whole file, with line 30 as it remembers it.
        let stale: String = (1..=40)
            .map(|n| if n == 30 { "line 30 with a heading it never saved\n".to_string() } else { format!("line {n} of the component\n") })
            .collect();
        let error = run(&w, "edit_file", json!({"path": "App.tsx", "old": stale, "new": "whatever\n"})).unwrap_err().to_string();
        assert!(error.contains("from line 1 to line 29, then differs at line 30"), "{error}");
        assert!(error.contains("the file has: line 30 of the component"), "{error}");
        assert!(error.contains("old has:      line 30 with a heading it never saved"), "{error}");
        assert!(error.contains("30: line 30 of the component"), "the lines around it, numbered: {error}");
        assert!(error.contains("40 lines long") && error.contains("write_file"), "{error}");
        assert_eq!(get(&w, "App.tsx"), file, "nothing written");
        let nothing = run(&w, "edit_file", json!({"path": "App.tsx", "old": "zzz qqq", "new": "x"})).unwrap_err().to_string();
        assert!(nothing.contains("no part of the file resembles it"), "{nothing}");
        // A short old text gets the short advice.
        let near = run(&w, "edit_file", json!({"path": "App.tsx", "old": "line 7 of the widget", "new": "x"})).unwrap_err().to_string();
        assert!(near.contains("Copy those lines") && !near.contains("write_file"), "{near}");
    }

    #[test]
    fn edits_keep_a_windows_files_line_endings() {
        let w = project("site");
        put(&w, "win.txt", "one\r\ntwo\r\nthree\r\n");
        run(&w, "edit_file", json!({"path": "win.txt", "old": "two", "new": "TWO\nand more"})).unwrap();
        assert_eq!(get(&w, "win.txt"), "one\r\nTWO\r\nand more\r\nthree\r\n");
        run(&w, "replace_lines", json!({"path": "win.txt", "from": 1, "to": 1, "text": "ONE"})).unwrap();
        assert_eq!(get(&w, "win.txt"), "ONE\r\nTWO\r\nand more\r\nthree\r\n", "replace_lines used to turn every ending into \\n");
        put(&w, "unix.txt", "one\ntwo\n");
        run(&w, "replace_lines", json!({"path": "unix.txt", "from": 2, "to": 2, "text": "TWO"})).unwrap();
        assert_eq!(get(&w, "unix.txt"), "one\nTWO\n");
    }

    #[test]
    fn replace_lines_takes_the_whole_block_as_expect_and_says_where_text_moved() {
        let w = project("site");
        let original = "import React from 'react';\n\n  // Filter data based on search\n  const filtered = items.filter(m =>\n    m.name.includes(query)\n  );\nexport default App;\n";
        put(&w, "App.tsx", original);
        // Every line of the range as expect, trailing space and all: accepted.
        run(&w, "replace_lines", json!({
            "path": "App.tsx", "from": 4, "to": 6,
            "expect": "  const filtered = items.filter(m => \n    m.name.includes(query)\n  );",
            "text": "  const filtered = items.filter(m => m.name.includes(query));"
        })).unwrap();
        assert!(get(&w, "App.tsx").contains("  // Filter data based on search\n  const filtered = items.filter(m => m.name.includes(query));\nexport"));
        // Numbers one off: the text is found where it is, and the numbers to send are named.
        put(&w, "App.tsx", original);
        let moved = run(&w, "replace_lines", json!({
            "path": "App.tsx", "from": 4, "to": 6,
            "expect": "  // Filter data based on search\n  const filtered = items.filter(m =>",
            "text": "x"
        })).unwrap_err().to_string();
        assert!(moved.contains("at line 3 now") && moved.contains("Send from 3 and to 5"), "{moved}");
        // Text not in the file: the real lines, and no claim that anything moved.
        let wrong = run(&w, "replace_lines", json!({"path": "App.tsx", "from": 4, "to": 4, "expect": "const nothing = here;", "text": "x"})).unwrap_err().to_string();
        assert!(wrong.contains("4:   const filtered") && !wrong.contains("have moved"), "{wrong}");
        assert_eq!(get(&w, "App.tsx"), original, "nothing replaced on a refused anchor");
        // Bounds.
        assert!(run(&w, "replace_lines", json!({"path": "App.tsx", "from": 0, "to": 1, "text": "x"})).unwrap_err().to_string().contains("start at 1"));
        assert!(run(&w, "replace_lines", json!({"path": "App.tsx", "from": 5, "to": 4, "text": "x"})).unwrap_err().to_string().contains("before"));
        assert!(run(&w, "replace_lines", json!({"path": "App.tsx", "from": 99, "to": 99, "text": "x"})).unwrap_err().to_string().contains("past its end"));
    }

    #[test]
    fn search_accepts_code_as_typed_and_names_files_from_the_project_root() {
        let w = project("site");
        put(&w, "src/components/Layout.tsx", "export const render = () => items[0];\n");
        put(&w, "node_modules/lib/index.js", "items[0]\n");
        let typed = run(&w, "search_text", json!({"query": "items[0", "path": "src"})).unwrap().output;
        assert!(typed.contains("plain text"), "{typed}");
        assert!(typed.contains("src/components/Layout.tsx:1:"), "named from the root, forward slashes: {typed}");
        let everywhere = run(&w, "search_text", json!({"query": "render"})).unwrap().output;
        assert!(everywhere.contains("src/components/Layout.tsx:1:"), "{everywhere}");
        assert!(!run(&w, "search_text", json!({"query": "items\\[0"})).unwrap().output.contains("node_modules"), "dependencies are skipped");
        let one = run(&w, "search_text", json!({"query": "render", "path": "src/components/Layout.tsx"})).unwrap().output;
        assert!(one.starts_with("src/components/Layout.tsx:1:"), "{one}");
        assert!(run(&w, "search_text", json!({"query": "absent_symbol"})).unwrap().output.contains("(no matches)"));
        let missing = run(&w, "search_text", json!({"query": "x", "path": "components"})).unwrap_err().to_string();
        assert!(missing.contains("src/components"), "{missing}");
    }

    #[test]
    fn outline_and_delete_point_at_the_real_path_and_refuse_what_they_cannot_do() {
        let w = project("site");
        put(&w, "src/app.py", "def main():\n    pass\n\nclass Store:\n    pass\n");
        let outline = run(&w, "outline", json!({"path": "src/app.py"})).unwrap().output;
        assert!(outline.contains("main") && outline.contains("Store"), "{outline}");
        let missing = run(&w, "outline", json!({"path": "app.py"})).unwrap_err().to_string();
        assert!(missing.contains("src/app.py"), "{missing}");
        let folder = run(&w, "delete_file", json!({"path": "src"})).unwrap_err().to_string();
        assert!(folder.contains("not a file"), "{folder}");
        let gone = run(&w, "delete_file", json!({"path": "app.py"})).unwrap_err().to_string();
        assert!(gone.contains("src/app.py"), "{gone}");
        run(&w, "delete_file", json!({"path": "src/app.py"})).unwrap();
        assert!(!w.root().join("src/app.py").exists());
        assert!(run(&w, "delete_file", json!({"path": "../x"})).is_err());
    }

    #[test]
    fn a_path_written_from_the_folder_above_resolves_inside_the_project() {
        let w = project("site");
        put(&w, "src/app.js", "let app;\n");
        // The earlier run's form of the path, and the project's own form.
        assert!(run(&w, "read_file", json!({"path": "site/src/app.js"})).unwrap().output.contains("let app;"));
        assert!(run(&w, "read_file", json!({"path": "src/app.js"})).unwrap().output.contains("let app;"));
        run(&w, "write_file", json!({"path": "site/src/new.js", "content": "new\n"})).unwrap();
        assert!(w.root().join("src/new.js").exists() && !w.root().join("site").exists(), "written inside, not into a site/site folder");
        assert_eq!(run(&w, "list_directory", json!({"path": "site"})).unwrap().output, "src/");
        assert_eq!(run(&w, "list_directory", json!({"path": "site\\src"})).unwrap().output, "app.js\nnew.js");
        // A project that holds a folder of its own name keeps it.
        let nested = project("app");
        put(&nested, "app/inner.txt", "inner\n");
        assert!(run(&nested, "read_file", json!({"path": "app/inner.txt"})).unwrap().output.contains("inner"));
        // Never outside the project.
        assert!(run(&w, "read_file", json!({"path": "site/../../escape.txt"})).is_err());
    }

    #[test]
    fn commands_report_output_and_exit_codes_and_refuse_servers_in_the_foreground() {
        let w = project("site");
        let ok = run(&w, "execute_command", json!({"command": "echo tool-check"})).unwrap();
        assert!(ok.output.contains("tool-check") && ok.exit_code == Some(0), "{}", ok.output);
        let failed = run(&w, "execute_command", json!({"command": "exit 3"})).unwrap();
        assert_eq!(failed.exit_code, Some(3), "{}", failed.output);
        let server = run(&w, "execute_command", json!({"command": "npm run dev"})).unwrap_err().to_string();
        assert!(server.contains("\"background\": true"), "{server}");
        std::fs::create_dir_all(w.root().join("sub")).unwrap();
        let inside = run(&w, "execute_command", json!({"command": "echo in-sub", "cwd": "sub"})).unwrap();
        assert!(inside.output.contains("in-sub"), "{}", inside.output);
        assert!(run(&w, "execute_command", json!({"command": "echo x", "cwd": "nope"})).is_err());
        assert!(run(&w, "execute_command", json!({"background_id": 999_999})).unwrap_err().to_string().contains("no background command"));
    }

    #[test]
    fn the_small_tools_answer_plainly() {
        let w = project("site");
        let system = run(&w, "system_info", json!({})).unwrap().output;
        assert!(system.contains("os=") && system.contains("arch="), "{system}");
        assert!(!run(&w, "list_processes", json!({})).unwrap().output.is_empty());
        let commit = run(&w, "git_commit", json!({"message": "x; rm -rf /"})).unwrap_err().to_string();
        assert!(commit.contains("single plain line"), "{commit}");
        let open = run(&w, "open_path", json!({"path": "nothing-here.txt"})).unwrap_err().to_string();
        assert!(open.contains("nothing was opened"), "{open}");
        // Unknown tools and approval.
        assert!(run(&w, "nuke", json!({})).is_err());
        let unapproved = execute(&ToolRequest { name: "write_file".into(), args: json!({"path": "a", "content": "b"}), approved: false }, &w, false);
        assert!(matches!(unapproved, Err(ToolError::PermissionRequired { .. })));
    }
}


#[cfg(test)]
mod command_checks {
    use super::*;
    use serde_json::json;

    fn project() -> WorkspaceManager {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "companion-command-checks-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        WorkspaceManager::new(dir)
    }

    fn run(w: &WorkspaceManager, args: serde_json::Value) -> ToolResult {
        execute(&ToolRequest { name: "execute_command".into(), args, approved: true }, w, true).unwrap()
    }

    #[test]
    fn a_command_with_double_quotes_reaches_the_program_as_written() {
        let w = project();
        // A program that prints its arguments one per line shows exactly what
        // arrived; python is what the live check used.
        let py = run(&w, json!({"command": "python -c \"import sys; print('|'.join(sys.argv[1:]))\" \"two words\" plain"}));
        if py.output.contains("not recognized") || py.output.contains("not found") {
            eprintln!("python is not on PATH here; skipping the python half");
        } else {
            assert_eq!(py.exit_code, Some(0), "{}", py.output);
            assert!(py.output.contains("two words|plain"), "{}", py.output);
        }
        // The shell's own quoting and operators still work.
        let echo = run(&w, json!({"command": "echo \"quoted text\" && echo second"}));
        assert_eq!(echo.exit_code, Some(0), "{}", echo.output);
        assert!(echo.output.contains("quoted text") && echo.output.contains("second"), "{}", echo.output);
        std::fs::create_dir_all(w.root().join("with space")).unwrap();
        std::fs::write(w.root().join("with space").join("note.txt"), "inside\n").unwrap();
        let spaced = if cfg!(windows) {
            run(&w, json!({"command": "type \"with space\\note.txt\""}))
        } else {
            run(&w, json!({"command": "cat \"with space/note.txt\""}))
        };
        assert!(spaced.output.contains("inside"), "{}", spaced.output);
    }

    #[test]
    fn stopping_a_background_command_as_a_shell_command_is_answered_with_the_right_form() {
        let w = project();
        #[cfg(windows)]
        let long = "ping -n 30 127.0.0.1 >nul";
        #[cfg(not(windows))]
        let long = "sleep 30";
        let started = run(&w, json!({"command": long, "background": true, "wait_secs": 1}));
        assert!(started.output.contains("Id "), "{}", started.output);
        let wrong = run(&w, json!({"command": "stop --background_id 1"}));
        assert!(wrong.output.contains("\"background_id\": N, \"stop\": true"), "{}", wrong.output);
        crate::terminal::stop_background_for(&w.root().to_string_lossy());
    }

    #[test]
    fn a_folder_given_to_a_write_says_so() {
        let w = project();
        std::fs::create_dir_all(w.root().join("site")).unwrap();
        let write = execute(&ToolRequest { name: "write_file".into(), args: json!({"path": "site", "content": "x"}), approved: true }, &w, true)
            .unwrap_err()
            .to_string();
        assert!(write.contains("is a folder") && write.contains("site/index.html"), "{write}");
        let append = execute(&ToolRequest { name: "append_file".into(), args: json!({"path": "site/", "content": "x"}), approved: true }, &w, true)
            .unwrap_err()
            .to_string();
        assert!(append.contains("is a folder"), "{append}");
    }
}
