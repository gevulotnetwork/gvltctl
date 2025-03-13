use anyhow::{anyhow, Context, Result};
use log::{error, trace};
use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

/// Run command returning decoded stdout and stderr.
pub fn run_command<I, S>(commands: I) -> Result<(String, String)>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let commands = commands
        .into_iter()
        .map(|s| (*s.as_ref()).to_os_string())
        .collect::<Vec<_>>();
    let program = &commands[0];
    let args = &commands[1..];

    trace!(
        "running command: '{} {}'",
        program.to_string_lossy(),
        args.join(OsStr::new(" ")).to_string_lossy()
    );

    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!(
            "failed to spawn child process for `{}`",
            program.to_string_lossy()
        ))?;

    // Log stdout and stderr of child process from separate threads
    let stdout = child.stdout.take().ok_or_else(|| {
        anyhow!(
            "failed to capture stdout of `{}`",
            program.to_string_lossy()
        )
    })?;

    let stderr = child.stderr.take().ok_or_else(|| {
        anyhow!(
            "failed to capture stderr of `{}`",
            program.to_string_lossy()
        )
    })?;

    let (stdout_tx, stdout_rx) = mpsc::channel::<String>();
    let (stderr_tx, stderr_rx) = mpsc::channel::<String>();
    let stdout_log_target = program.to_string_lossy().to_string();
    let stderr_log_target = program.to_string_lossy().to_string();

    let stdout_thread = thread::spawn(move || {
        BufReader::new(stdout)
            .lines()
            .filter_map(Result::ok)
            .for_each(|line| {
                trace!(target: stdout_log_target.as_str(), "{}", line);
                stdout_tx.send(line).unwrap();
            });
    });

    let stderr_thread = thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .filter_map(Result::ok)
            .for_each(|line| {
                trace!(target: stderr_log_target.as_str(), "{}", line);
                stderr_tx.send(line).unwrap();
            });
    });

    let exit_status = child.wait().context(format!(
        "attempted to wait for child process `{}` which is not running",
        program.to_string_lossy()
    ))?;

    stdout_thread.join().unwrap();
    stderr_thread.join().unwrap();

    let stdout = stdout_rx.into_iter().collect::<Vec<String>>().join("\n");
    let stderr = stderr_rx.into_iter().collect::<Vec<String>>().join("\n");

    if exit_status.success() {
        Ok((stdout, stderr))
    } else {
        stderr
            .lines()
            .for_each(|line| error!(target: &program.to_string_lossy(), "{}", line));
        Err(anyhow!(
            "child process `{}` failed with {}",
            program.to_string_lossy(),
            exit_status
        ))
    }
}
