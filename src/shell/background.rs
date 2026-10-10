//! Process-owned background capture. No PTY, arbitrary PID API or persistent handles.
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    collections::VecDeque,
    ffi::OsString,
    io::Read,
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::watch;

const MAX_JOBS: usize = 16;
const BUFFER_BYTES: usize = 1024 * 1024;

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
struct Buffer {
    bytes: VecDeque<u8>,
    total: u64,
}
impl Buffer {
    fn push(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len() as u64);
        let bytes = &bytes[bytes.len().saturating_sub(BUFFER_BYTES)..];
        let excess = (self.bytes.len() + bytes.len()).saturating_sub(BUFFER_BYTES);
        self.bytes.drain(..excess);
        self.bytes.extend(bytes);
    }
    fn page(&self, offset: u64, limit: usize) -> Result<OutputPage> {
        if offset > self.total {
            bail!("offset is beyond available output")
        }
        let first = self.total - self.bytes.len() as u64;
        let start = offset.max(first);
        let bytes: Vec<_> = self
            .bytes
            .iter()
            .skip((start - first) as usize)
            .take(limit)
            .copied()
            .collect();
        Ok(OutputPage {
            offset: start,
            next_offset: start + bytes.len() as u64,
            available_offset: first,
            total_bytes: self.total,
            truncated: offset < first,
            // Offsets count raw pipe bytes, independently of UTF-8 decoding and filtering.
            text: super::filter_unsafe_ansi(&String::from_utf8_lossy(&bytes)),
        })
    }
}

/// One stream page. Offsets refer to original bytes, not decoded text lengths.
#[derive(Debug, Serialize)]
pub struct OutputPage {
    /// Actual starting byte offset (may advance when old bytes were evicted).
    pub offset: u64,
    /// Offset to use for the next read of this stream.
    pub next_offset: u64,
    /// Earliest retained byte offset.
    pub available_offset: u64,
    /// Total bytes received so far.
    pub total_bytes: u64,
    /// Requested bytes were evicted from the bounded tail buffer.
    pub truncated: bool,
    /// Lossy UTF-8 text with unsafe terminal controls removed.
    pub text: String,
}

#[derive(Default)]
struct State {
    stdout: Buffer,
    stderr: Buffer,
    finished: bool,
    exit_code: Option<i32>,
    signal: Option<i32>,
    stopped: bool,
    timed_out: bool,
    error: Option<String>,
}
struct Job {
    id: String,
    scope: OsString,
    cancel: watch::Sender<bool>,
    state: Mutex<State>,
    created: Instant,
    ui_lease: Mutex<Option<Arc<crate::runtime::resources::UiLease>>>,
}

/// Snapshot of a managed child and its independently paged stdout/stderr.
#[derive(Debug, Serialize)]
pub struct BackgroundOutput {
    /// Random process-local handle, never an operating system PID.
    pub child_id: String,
    /// True after process cleanup, wait and final output collection.
    pub finished: bool,
    /// Representable exit code, otherwise null.
    pub exit_code: Option<i32>,
    /// Signal that terminated the child, otherwise null.
    pub signal: Option<i32>,
    /// A stop request was processed.
    pub stopped: bool,
    /// Background runtime limit was reached.
    pub timed_out: bool,
    /// Capture or cleanup error, when present.
    pub error: Option<String>,
    /// Standard output page.
    pub stdout: OutputPage,
    /// Standard error page.
    pub stderr: OutputPage,
}

struct Registry {
    jobs: Vec<Arc<Job>>,
    stopping: bool,
}
fn registry() -> &'static Mutex<Registry> {
    static VALUE: OnceLock<Mutex<Registry>> = OnceLock::new();
    VALUE.get_or_init(|| {
        Mutex::new(Registry {
            jobs: Vec::new(),
            stopping: false,
        })
    })
}

pub(crate) fn scope(config: &crate::config::Config) -> Result<OsString> {
    let path = match &config.source {
        Some(path) => path.clone(),
        None => crate::config::default_config_path()?,
    };
    // Existing configuration aliases resolve to the same ownership domain.
    Ok(std::fs::canonicalize(&path)
        .unwrap_or(path)
        .into_os_string())
}
fn find(scope: &OsString, id: &str) -> Result<Arc<Job>> {
    lock(registry())
        .jobs
        .iter()
        .find(|job| job.scope == *scope && job.id == id)
        .cloned()
        .context("unknown or expired background child_id in this configuration/process")
}

pub(crate) fn spawn(
    scope: OsString,
    program: OsString,
    args: Vec<OsString>,
    timeout_secs: u64,
    ui_lease: Option<Arc<crate::runtime::resources::UiLease>>,
) -> Result<String> {
    if !(1..=86400).contains(&timeout_secs) {
        bail!("background_timeout_secs must be 1–86400")
    }
    let mut registry = lock(registry());
    if registry.stopping {
        bail!("background executor is shutting down")
    }
    if registry.jobs.len() >= MAX_JOBS {
        let oldest = registry
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| lock(&job.state).finished)
            .min_by_key(|(_, job)| job.created)
            .map(|(index, _)| index);
        if let Some(index) = oldest {
            registry.jobs.remove(index);
        } else {
            bail!("background child limit reached (16)")
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (cancel, receiver) = watch::channel(false);
    let job = Arc::new(Job {
        id: id.clone(),
        scope,
        cancel,
        state: Mutex::new(State::default()),
        created: Instant::now(),
        ui_lease: Mutex::new(ui_lease),
    });
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().context("cannot spawn background shell")?;
    let guard = ChildGuard(Some(child));
    let owned = job.clone();
    std::thread::Builder::new()
        .name("shell-background".into())
        .spawn(move || {
            let result = supervise(guard, &owned, receiver, timeout_secs);
            lock(&owned.ui_lease).take();
            let mut state = lock(&owned.state);
            if let Err(error) = result {
                state.error = Some(format!("{error:#}"));
            }
            state.finished = true;
        })
        .context("cannot start background supervisor")?;
    registry.jobs.push(job);
    Ok(id)
}

pub(crate) fn read(
    scope: &OsString,
    id: &str,
    offset: u64,
    stderr_offset: u64,
    limit: usize,
) -> Result<BackgroundOutput> {
    if !(1..=16384).contains(&limit) {
        bail!("max_bytes must be 1–16384 per stream")
    }
    snapshot(find(scope, id)?.as_ref(), offset, stderr_offset, limit)
}
fn snapshot(job: &Job, offset: u64, stderr_offset: u64, limit: usize) -> Result<BackgroundOutput> {
    let state = lock(&job.state);
    Ok(BackgroundOutput {
        child_id: job.id.clone(),
        finished: state.finished,
        exit_code: state.exit_code,
        signal: state.signal,
        stopped: state.stopped,
        timed_out: state.timed_out,
        error: state.error.clone(),
        stdout: state.stdout.page(offset, limit)?,
        stderr: state.stderr.page(stderr_offset, limit)?,
    })
}
pub(crate) async fn stop(scope: &OsString, id: &str) -> Result<BackgroundOutput> {
    let job = find(scope, id)?;
    job.cancel.send_replace(true);
    wait_finished(&job).await?;
    snapshot(&job, 0, 0, 1024)
}
async fn wait_finished(job: &Job) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !lock(&job.state).finished {
        if tokio::time::Instant::now() >= deadline {
            bail!("background cleanup pending; read_output to inspect status")
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

/// Stops and reaps all process-owned background shells before runtime shutdown.
/// Embedders must call this before dropping their runtime; it rejects future starts.
pub async fn shutdown_background() -> Result<()> {
    let jobs = {
        let mut registry = lock(registry());
        registry.stopping = true;
        registry.jobs.clone()
    };
    for job in &jobs {
        job.cancel.send_replace(true);
    }
    for job in &jobs {
        wait_finished(job).await?;
    }
    Ok(())
}

// Until cleanup is complete, the unreaped group leader pins the PID/PGID identity.
struct ChildGuard(Option<Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = signal(child.id(), libc::SIGKILL);
            let _ = child.wait();
        }
    }
}
fn signal(pid: u32, sig: i32) -> Result<()> {
    if unsafe { libc::kill(-(pid as i32), sig) } < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error).context("cannot signal background process group");
        }
    }
    Ok(())
}
fn exited(pid: u32) -> Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error).context("cannot inspect background child status");
    }
    Ok(unsafe { info.si_pid() } != 0)
}
fn nonblocking(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error())
            .context("cannot set background pipe nonblocking");
    }
    Ok(())
}
fn drain<R: Read>(reader: &mut R, job: &Job, stderr: bool) -> Result<bool> {
    let mut bytes = [0; 4096];
    // Bound each pass so a noisy stream cannot starve cancellation/status checks.
    for _ in 0..16 {
        match reader.read(&mut bytes) {
            Ok(0) => return Ok(true),
            Ok(count) => {
                let mut state = lock(&job.state);
                if stderr {
                    state.stderr.push(&bytes[..count]);
                } else {
                    state.stdout.push(&bytes[..count]);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("background output read failed"),
        }
    }
    Ok(false)
}
fn supervise(
    mut guard: ChildGuard,
    job: &Job,
    cancel: watch::Receiver<bool>,
    timeout_secs: u64,
) -> Result<()> {
    use std::os::unix::process::ExitStatusExt;
    let child = guard.0.as_mut().context("background child missing")?;
    let pid = child.id();
    let mut stdout = child.stdout.take().context("background stdout missing")?;
    let mut stderr = child.stderr.take().context("background stderr missing")?;
    nonblocking(stdout.as_raw_fd())?;
    nonblocking(stderr.as_raw_fd())?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut terminating = None;
    loop {
        drain(&mut stdout, job, false)?;
        drain(&mut stderr, job, true)?;
        let done = exited(pid)?;
        if terminating.is_none() && (done || *cancel.borrow() || Instant::now() >= deadline) {
            let mut state = lock(&job.state);
            state.stopped = *cancel.borrow();
            state.timed_out = !done && Instant::now() >= deadline;
            drop(state);
            // Clean descendants even if the shell exits first or closes its pipes.
            signal(pid, libc::SIGTERM)?;
            terminating = Some(Instant::now());
        }
        if terminating.is_some_and(|time| time.elapsed() >= Duration::from_millis(500)) {
            signal(pid, libc::SIGKILL)?;
            let status = child.wait().context("cannot reap background shell")?;
            guard.0 = None; // Never signal a PID after wait has released its identity.
            let drain_deadline = Instant::now() + Duration::from_millis(500);
            let complete = loop {
                let out_eof = drain(&mut stdout, job, false)?;
                let err_eof = drain(&mut stderr, job, true)?;
                if out_eof && err_eof {
                    break true;
                }
                if Instant::now() >= drain_deadline {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let mut state = lock(&job.state);
            if !complete {
                state.error = Some("output pipes did not close after group cleanup; escaped descendants are unsupported and capture may be incomplete".into());
            }
            state.exit_code = status.code();
            state.signal = status.signal();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_use_raw_offsets_and_report_eviction() -> Result<()> {
        let mut buffer = Buffer::default();
        buffer.push(&vec![b'x'; BUFFER_BYTES + 5]);
        buffer.push("中\x1b[2Jz".as_bytes());
        let page = buffer.page(0, 20)?;
        assert!(page.truncated);
        assert_eq!(page.offset, 13);
        assert_eq!(page.next_offset, 33);
        let page = buffer.page(buffer.total - 8, 20)?;
        assert_eq!(page.text, "中z");
        assert_eq!(page.next_offset, buffer.total);
        assert!(buffer.page(buffer.total + 1, 1).is_err());
        Ok(())
    }

    fn start(source: &str, seconds: u64) -> Result<(OsString, String)> {
        let scope = OsString::from(uuid::Uuid::new_v4().to_string());
        let shell = if cfg!(target_os = "android") {
            "/system/bin/sh"
        } else {
            "/bin/sh"
        };
        let id = spawn(
            scope.clone(),
            shell.into(),
            vec!["-c".into(), source.into()],
            seconds,
            None,
        )?;
        Ok((scope, id))
    }

    #[tokio::test]
    async fn starts_immediately_captures_both_streams_and_stops() -> Result<()> {
        let started = Instant::now();
        let (scope, id) = start("printf out; printf err >&2; sleep 30", 30)?;
        assert!(started.elapsed() < Duration::from_secs(2));
        let job = find(&scope, &id)?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let result = read(&scope, &id, 0, 0, 10)?;
            if result.stdout.text == "out" && result.stderr.text == "err" {
                break;
            }
            anyhow::ensure!(Instant::now() < deadline, "output not captured");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(read(&OsString::from("different"), &id, 0, 0, 10).is_err());
        let output = stop(&scope, &id).await?;
        assert!(output.finished && output.stopped && !output.timed_out);
        assert!(output.error.is_none(), "{:?}", output.error);
        assert!(stop(&scope, &id).await?.finished);
        assert!(lock(&job.state).finished);
        Ok(())
    }

    #[tokio::test]
    async fn timeout_kills_term_ignoring_child_even_with_closed_pipes() -> Result<()> {
        let (scope, id) = start("trap '' TERM; exec 1>&- 2>&-; while :; do sleep 1; done", 1)?;
        wait_finished(find(&scope, &id)?.as_ref()).await?;
        let output = read(&scope, &id, 0, 0, 10)?;
        assert!(output.finished && output.timed_out);
        assert_eq!(output.signal, Some(libc::SIGKILL));
        assert!(output.error.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn natural_exit_preserves_status_and_cleans_pipe_holding_descendants() -> Result<()> {
        let (scope, id) = start("sleep 30 & printf done; exit 7", 30)?;
        let job = find(&scope, &id)?;
        wait_finished(&job).await?;
        let output = read(&scope, &id, 0, 0, 10)?;
        assert_eq!(output.exit_code, Some(7));
        assert_eq!(output.stdout.text, "done");
        assert!(output.error.is_none(), "{:?}", output.error);
        assert!(!output.stopped && !output.timed_out);
        Ok(())
    }

    #[tokio::test]
    async fn high_volume_capture_drains_and_retains_bounded_tail() -> Result<()> {
        let (scope, id) = start(
            "head -c 2097152 /dev/zero; printf tail; head -c 2097152 /dev/zero >&2",
            10,
        )?;
        let job = find(&scope, &id)?;
        wait_finished(&job).await?;
        let output = read(&scope, &id, 0, 0, 10)?;
        assert_eq!(output.exit_code, Some(0));
        assert!(output.stdout.truncated && output.stderr.truncated);
        assert_eq!(output.stdout.total_bytes, 2097156);
        assert_eq!(output.stderr.total_bytes, 2097152);
        let state = lock(&job.state);
        assert_eq!(state.stdout.bytes.len(), BUFFER_BYTES);
        assert_eq!(state.stderr.bytes.len(), BUFFER_BYTES);
        Ok(())
    }
}
