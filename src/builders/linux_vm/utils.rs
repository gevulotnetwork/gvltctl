use anyhow::{anyhow, Context, Result};
use log::{debug, trace};
use std::ffi::OsStr;
use std::fmt;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

pub fn run_command<I, S>(commands: I, as_root: bool) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr> + Clone + fmt::Debug,
{
    let commands = commands.into_iter().collect::<Vec<_>>();
    let program = if as_root {
        OsStr::new("sudo")
    } else {
        commands[0].as_ref()
    };
    let args = if as_root {
        commands.as_slice()
    } else {
        &commands[1..]
    };

    debug!("running command: {} {:?}", program.to_string_lossy(), &args);

    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn command")?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Could not capture stdout."))?;

    let reader = BufReader::new(stdout);
    reader
        .lines()
        .filter_map(|line| line.ok())
        .for_each(|line| trace!(target: &commands[0].as_ref().to_string_lossy(), "{}", line));

    let output = child
        .wait_with_output()
        .context("Failed to wait for command")?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout).context("Failed to parse command stdout")?)
    } else {
        String::from_utf8(output.stderr)
            .context("Failed to parse command stderr")?
            .lines()
            .for_each(|line| debug!(target: &commands[0].as_ref().to_string_lossy(), "{}", line));
        Err(anyhow!("Command failed with status {}", output.status))
    }
}
