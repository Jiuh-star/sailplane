//! Locating the running `headscale serve` process and signaling it.
//!
//! Shared by the `proc` and `kubernetes` integrations. Headscale re-reads its
//! configuration on `SIGHUP`, which avoids a restart.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::headscale::Headscale;

/// How long to wait for Headscale to report healthy after a reload.
const HEALTH_ATTEMPTS: usize = 10;
const HEALTH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// Finds the PID of the `headscale serve` process by scanning `/proc`.
///
/// Returns the first match. Headscale is a single process in every supported
/// deployment shape.
pub fn find_headscale_pid(proc_root: &Path) -> Option<u32> {
    let entries = std::fs::read_dir(proc_root).ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };

        let comm = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
        if comm.trim() != "headscale" {
            continue;
        }

        // `headscale serve` is the long-running server. The CLI exits.
        let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        if cmdline.split_whitespace().any(|arg| arg == "serve") {
            return Some(pid);
        }
    }

    None
}

/// Sends `SIGHUP` to a process.
pub fn signal_reload(pid: u32) -> Result<()> {
    // SAFETY: `kill` is async-signal-safe and we pass a PID we just discovered.
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGHUP) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        bail!("failed to send SIGHUP to PID {pid}: {err}");
    }
    Ok(())
}

/// Reloads Headscale by signaling `headscale serve` and waiting for health.
pub async fn reload_via_signal(pid: u32, headscale: &Headscale) -> Result<()> {
    signal_reload(pid).context("failed to signal the Headscale process")?;

    for attempt in 1..=HEALTH_ATTEMPTS {
        if headscale.health().await {
            return Ok(());
        }
        if attempt < HEALTH_ATTEMPTS {
            tokio::time::sleep(HEALTH_INTERVAL).await;
        }
    }

    bail!(
        "Headscale did not report healthy within {}s of the reload signal",
        HEALTH_ATTEMPTS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_proc_root_returns_none() {
        assert_eq!(find_headscale_pid(Path::new("/definitely/not/here")), None);
    }

    #[test]
    fn finds_a_synthetic_headscale_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_dir = dir.path().join("4242");
        std::fs::create_dir_all(&pid_dir).unwrap();
        std::fs::write(pid_dir.join("comm"), "headscale\n").unwrap();
        std::fs::write(pid_dir.join("cmdline"), "headscale\0serve\0").unwrap();

        assert_eq!(find_headscale_pid(dir.path()), Some(4242));
    }

    #[test]
    fn ignores_processes_not_running_serve() {
        let dir = tempfile::tempdir().unwrap();
        let pid_dir = dir.path().join("7");
        std::fs::create_dir_all(&pid_dir).unwrap();
        std::fs::write(pid_dir.join("comm"), "headscale\n").unwrap();
        std::fs::write(pid_dir.join("cmdline"), "headscale\0users\0list\0").unwrap();

        assert_eq!(find_headscale_pid(dir.path()), None);
    }

    #[test]
    fn ignores_other_processes() {
        let dir = tempfile::tempdir().unwrap();
        let pid_dir = dir.path().join("9");
        std::fs::create_dir_all(&pid_dir).unwrap();
        std::fs::write(pid_dir.join("comm"), "nginx\n").unwrap();
        std::fs::write(pid_dir.join("cmdline"), "nginx\0serve\0").unwrap();

        assert_eq!(find_headscale_pid(dir.path()), None);
    }
}
