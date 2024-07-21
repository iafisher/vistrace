use anyhow::{anyhow, Result};
use std::io::Read;
use std::process::{Command, Stdio};
use std::{env, process};

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(trailing_var_arg = true)]
struct Args {
    /// passed on to strace
    #[arg(required = true, num_args = 1..)]
    args: Vec<String>,
}

fn main() {
    let result = main_can_err();
    if let Err(e) = result {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}

fn main_can_err() -> Result<()> {
    ensure_linux();
    let args = Args::parse();
    let mut cmd: std::process::Child = Command::new("strace")
        .args(args.args)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("unable to spawn strace: {}", e))?;
    let stderr = cmd
        .stderr
        .as_mut()
        .ok_or(anyhow!("unable to access strace's standard error"))?;

    // TODO: optimal buffer size?
    let mut buffer = [0; 512];
    loop {
        let n = stderr
            .read(&mut buffer)
            .map_err(|e| anyhow!("unable to read output from strace: {}", e))?;
        if n == 0 {
            break;
        }
        // TODO: will strace ever print non-ASCII bytes? what happens if, e.g., syscall arg is non-UTF-8 string?
        let s = std::str::from_utf8(&buffer[0..n])
            .map_err(|e| anyhow!("could not decode strace output as UTF-8: {}", e))?;
        print!("{}", s);
    }

    let exit_result = cmd.wait().map_err(|e| anyhow!("failed to wait for strace to terminate: {}", e))?;
    if !exit_result.success() {
        return Err(anyhow!("strace returned a non-zero exit code"));
    }
    Ok(())
}

fn ensure_linux() {
    let os = env::consts::OS;
    if os != "linux" {
        eprintln!(
            "This command only works on Linux (detected OS: {}). Sorry.",
            os
        );
        process::exit(1);
    }
}
