//! Companion's private PostgreSQL (Phase 1, task 1). Without
//! COMPANION_DATABASE_URL, Companion runs its own database server from
//! runtime/pgsql (installed by scripts/get-postgres.ps1): its files in the
//! data folder, listening only on 127.0.0.1, with a password generated when
//! the files were created. It starts with Companion and stops when Companion
//! closes; one left running by a Companion that was ended abruptly is found
//! and used again.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The database user, and the database Companion keeps its history in.
const USER: &str = "companion";

#[derive(Clone, Debug)]
pub struct Cluster {
    bin: PathBuf,
    dir: PathBuf,
    port: u16,
    password: String,
    /// False when the server was already running and was only found.
    pub started: bool,
}

fn tool(bin: &Path, name: &str) -> Command {
    let mut command = Command::new(bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)));
    // Never a pipe: the server inherits pg_ctl's handles and would hold a pipe
    // open for as long as it runs.
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // A build from source has no fixed library path (it can be moved).
    #[cfg(unix)]
    command.env("LD_LIBRARY_PATH", bin.join("../lib"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

impl Cluster {
    /// The server for the database files in `dir`, running: the files are
    /// created on first use, and the server started unless it already runs.
    /// `log` receives the server's own messages; `settings` are server
    /// settings for this start (`name=value`).
    pub fn start(bin: &Path, dir: &Path, log: &Path, settings: &[&str]) -> Result<Cluster, String> {
        if !bin.join(format!("pg_ctl{}", std::env::consts::EXE_SUFFIX)).is_file() {
            return Err(format!(
                "PostgreSQL is not installed in {}: run scripts/get-postgres.ps1, or set COMPANION_DATABASE_URL to a PostgreSQL database",
                bin.display()
            ));
        }
        let secret = dir.with_extension("secret");
        if !dir.join("PG_VERSION").is_file() {
            create(bin, dir, &secret)?;
        }
        let password = std::fs::read_to_string(&secret)
            .map_err(|e| format!("cannot read the database password {}: {e}", secret.display()))?
            .trim()
            .to_string();
        let mut cluster = Cluster { bin: bin.to_path_buf(), dir: dir.to_path_buf(), port: 0, password, started: false };
        if let Some(port) = cluster.running_port() {
            cluster.port = port;
            return Ok(cluster);
        }
        // ponytail: the port can be taken before the server binds it; that
        // start fails and the next one picks another.
        cluster.port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .map_err(|e| format!("no free local port for the database: {e}"))?
            .port();
        if let Some(parent) = log.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let status = tool(bin, "pg_ctl")
            .args(["start", "-w", "-t", "60", "-D"])
            .arg(dir)
            .arg("-l")
            .arg(log)
            .arg("-o")
            .arg(format!("-p {} -h 127.0.0.1{}", cluster.port, settings.iter().map(|s| format!(" -c {s}")).collect::<String>()))
            .status()
            .map_err(|e| format!("cannot run pg_ctl: {e}"))?;
        if !status.success() {
            let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
            let last: Vec<&str> = text.lines().rev().take(5).collect();
            return Err(format!(
                "the database server did not start ({status}); its log {} ends: {}",
                log.display(),
                last.into_iter().rev().collect::<Vec<_>>().join(" | ")
            ));
        }
        cluster.started = true;
        Ok(cluster)
    }

    /// The port of the server running on these files, if one is.
    fn running_port(&self) -> Option<u16> {
        let status = tool(&self.bin, "pg_ctl").arg("status").arg("-D").arg(&self.dir).status().ok()?;
        if !status.success() {
            return None;
        }
        // postmaster.pid: pid, folder, start time, port, ...
        std::fs::read_to_string(self.dir.join("postmaster.pid")).ok()?.lines().nth(3)?.trim().parse().ok()
    }

    pub fn url(&self, database: &str) -> String {
        format!("postgres://{USER}:{}@127.0.0.1:{}/{database}", self.password, self.port)
    }

    /// The URL of Companion's database, created if it is not there yet.
    pub async fn companion_database(&self) -> Result<String, sqlx::Error> {
        use sqlx::Connection;
        let mut admin = sqlx::PgConnection::connect(&self.url("postgres")).await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(USER)
            .fetch_one(&mut admin)
            .await?;
        if !exists {
            sqlx::query("CREATE DATABASE companion").execute(&mut admin).await?;
        }
        Ok(self.url(USER))
    }

    /// Tests on unix, which has no job objects: a watcher stops this server
    /// once the process `pid` has ended.
    #[cfg(all(test, unix))]
    pub fn stop_after(&self, pid: u32) {
        let _ = Command::new("sh")
            .arg("-c")
            .arg("while kill -0 \"$0\" 2>/dev/null; do sleep 1; done; exec \"$1\" stop -m fast -D \"$2\"")
            .arg(pid.to_string())
            .arg(self.bin.join("pg_ctl"))
            .arg(&self.dir)
            .env("LD_LIBRARY_PATH", self.bin.join("../lib"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    /// Stop the server, letting it finish writing first.
    pub fn stop(&self) {
        let status = tool(&self.bin, "pg_ctl").args(["stop", "-m", "fast", "-w", "-t", "30", "-D"]).arg(&self.dir).status();
        match status {
            Ok(status) if status.success() => tracing::info!("stopped the private database"),
            other => tracing::warn!(?other, "the private database did not stop; the next start uses it again"),
        }
    }
}

/// The database at `url` copied into `file` by pg_dump (the one in `bin`,
/// else the one on PATH); pg_restore puts it back. The password goes to
/// pg_dump in its environment, never on a command line others can read.
pub fn dump(bin: &Path, url: &str, file: &Path) -> Result<(), String> {
    let mut url = reqwest::Url::parse(url).map_err(|e| format!("not a database URL: {e}"))?;
    let password = percent_encoding::percent_decode_str(url.password().unwrap_or_default()).decode_utf8_lossy().into_owned();
    let _ = url.set_password(None);
    let program = if bin.join(format!("pg_dump{}", std::env::consts::EXE_SUFFIX)).is_file() { bin } else { Path::new("") };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let output = tool(program, "pg_dump")
        .stderr(Stdio::piped())
        .env("PGPASSWORD", password)
        .arg("--format=custom")
        .arg(format!("--file={}", file.display()))
        .arg(format!("--dbname={url}"))
        .output()
        .map_err(|e| format!("cannot run pg_dump: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

/// New database files in `dir`, with a generated password kept in `secret`.
fn create(bin: &Path, dir: &Path, secret: &Path) -> Result<(), String> {
    // Two random UUIDs: 244 random bits, as hex, so the URL needs no escaping.
    let password = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    if let Some(parent) = secret.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(secret, &password).map_err(|e| format!("cannot write {}: {e}", secret.display()))?;
    let output = tool(bin, "initdb")
        .stderr(Stdio::piped())
        .arg("-D")
        .arg(dir)
        .args(["-U", USER, "-A", "scram-sha-256", "-E", "UTF8", "--locale=C", "--no-instructions"])
        .arg(format!("--pwfile={}", secret.display()))
        .output()
        .map_err(|e| format!("cannot run initdb: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot create the database files in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    tracing::info!(dir = %dir.display(), "created the private database");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_running_server_is_found_again_not_started_twice() {
        if std::env::var("COMPANION_TEST_DATABASE_URL").is_ok_and(|url| !url.trim().is_empty()) {
            return; // the tests use that server, not a private one
        }
        let first = crate::storage::testing::private_server();
        let again = super::Cluster::start(&first.bin, &first.dir, &first.dir.with_extension("log"), &[]).unwrap();
        assert!(!again.started);
        assert_eq!(again.url("postgres"), first.url("postgres"));
    }

    #[test]
    fn a_backup_is_written() {
        if std::env::var("COMPANION_TEST_DATABASE_URL").is_ok_and(|url| !url.trim().is_empty()) {
            return;
        }
        let server = crate::storage::testing::private_server();
        let file = std::env::temp_dir().join(format!("companion-backup-{}.dump", uuid::Uuid::new_v4().simple()));
        super::dump(&server.bin, &server.url("postgres"), &file).unwrap();
        assert!(std::fs::read(&file).unwrap().starts_with(b"PGDMP"), "a pg_restore archive");
        let _ = std::fs::remove_file(file);
    }
}
