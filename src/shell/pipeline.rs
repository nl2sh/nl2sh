use super::{process, ExecutionRequest, ExecutionResult};
use crate::limits::BoundedText;
use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::{
    io::{AsyncReadExt, BufReader},
    process::Command,
    sync::watch,
    time::Duration,
};
pub async fn execute(req: ExecutionRequest) -> Result<ExecutionResult> {
    if req.cancel.as_ref().is_some_and(|cancel| *cancel.borrow()) {
        anyhow::bail!("command cancelled before execution");
    }
    let mut cmd = Command::new(&req.program);
    cmd.args(&req.args)
        .stdin(if req.interactive {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().context("failed to spawn shell")?;
    let pid = child.id().context("child has no pid")?;
    let out = child.stdout.take().context("missing stdout")?;
    let err = child.stderr.take().context("missing stderr")?;
    let stdout_task = tokio::spawn(read(out, false, req.capture_max_bytes, req.output.clone()));
    let stderr_task = tokio::spawn(read(err, true, req.capture_max_bytes, req.output.clone()));
    enum End {
        Status(std::process::ExitStatus),
        Timeout,
        Interrupted,
        Cancelled,
    }
    let end = if req.timeout_secs == 0 {
        tokio::select! {
            status = child.wait() => End::Status(status?),
            signal = tokio::signal::ctrl_c() => { signal?; End::Interrupted }
            _ = cancelled(req.cancel.clone()) => End::Cancelled,
        }
    } else {
        tokio::select! {
            status = child.wait() => End::Status(status?),
            _ = tokio::time::sleep(Duration::from_secs(req.timeout_secs)) => End::Timeout,
            signal = tokio::signal::ctrl_c() => { signal?; End::Interrupted }
            _ = cancelled(req.cancel.clone()) => End::Cancelled,
        }
    };
    let (timed, interrupted, status) = match end {
        End::Status(status) => (false, false, Some(status)),
        End::Timeout | End::Interrupted | End::Cancelled => {
            let interrupted = matches!(end, End::Interrupted | End::Cancelled);
            if interrupted {
                process::signal_group(pid, nix::sys::signal::Signal::SIGINT);
                tokio::time::sleep(Duration::from_millis(250)).await;
                if child.try_wait()?.is_none() {
                    process::signal_group(pid, nix::sys::signal::Signal::SIGTERM);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            } else {
                process::signal_group(pid, nix::sys::signal::Signal::SIGTERM);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if child.try_wait()?.is_none() {
                process::signal_group(pid, nix::sys::signal::Signal::SIGKILL);
            }
            let status = child.wait().await.ok();
            (!interrupted, interrupted, status)
        }
    };
    let stdout = stdout_task.await.context("stdout reader task failed")?;
    let stderr = stderr_task.await.context("stderr reader task failed")?;
    Ok(ExecutionResult {
        stdout,
        stderr,
        exit_code: status.and_then(|s| s.code()),
        timed_out: timed,
        interrupted,
    })
}
async fn cancelled(mut signal: Option<watch::Receiver<bool>>) {
    let Some(ref mut receiver) = signal else {
        std::future::pending::<()>().await;
        return;
    };
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}
async fn read<R: tokio::io::AsyncRead + Unpin>(
    r: R,
    e: bool,
    max_bytes: usize,
    output: std::sync::Arc<dyn super::OutputSink>,
) -> String {
    let mut r = BufReader::new(r);
    let mut b = [0; 4096];
    let mut captured = BoundedText::new(max_bytes);
    loop {
        match r.read(&mut b).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let text = super::filter_unsafe_ansi(&String::from_utf8_lossy(&b[..n]));
                if e {
                    output.stderr(&text)
                } else {
                    output.stdout(&text)
                }
                captured.push(text.as_bytes());
            }
        }
    }
    captured.finish()
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;
    use crate::shell::NullOutput;
    use std::{ffi::OsString, sync::Arc};

    #[tokio::test]
    async fn cancellation_reaps_captured_process_group() -> Result<()> {
        let (sender, receiver) = watch::channel(false);
        let request = ExecutionRequest {
            program: OsString::from("/bin/sh"),
            args: vec![OsString::from("-c"), OsString::from("sleep 30")],
            timeout_secs: 30,
            use_pty: false,
            interactive: false,
            output: Arc::new(NullOutput),
            capture_max_bytes: 1024,
            tui_active: false,
            tui_suspended: None,
            cancel: Some(receiver),
        };
        let task = tokio::spawn(execute(request));
        tokio::time::sleep(Duration::from_millis(100)).await;
        sender.send_replace(true);
        let result = tokio::time::timeout(Duration::from_secs(5), task).await???;
        assert!(result.interrupted);
        assert!(!result.timed_out);
        Ok(())
    }
}
