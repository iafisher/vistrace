use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use anyhow::{anyhow, Result};

pub struct Syscall {
    pub text: String,
}

pub fn strace(cmd: &Vec<String>, tx: mpsc::Sender<Syscall>) -> Result<()> {
    let mut child: std::process::Child = Command::new("strace")
        .args(cmd)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("unable to spawn strace: {}", e))?;
    let stderr = child
        .stderr
        .as_mut()
        .ok_or(anyhow!("unable to access strace's standard error"))?;

    let mut reader = BufReader::new(stderr);

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| anyhow!("unable to read output from strace: {}", e))?;
        if n == 0 {
            break;
        }

        if line.starts_with("+++") {
            continue;
        }

        tx.send(Syscall { text: line }).map_err(|e| anyhow!("transmit error: {}", e))?;
    }

    let exit_result = child.wait().map_err(|e| anyhow!("failed to wait for strace to terminate: {}", e))?;
    if !exit_result.success() {
        return Err(anyhow!("strace returned a non-zero exit code"));
    }
    Ok(())
}
