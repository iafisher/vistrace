use std::io::Read;
use std::process::{Command, Stdio};

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(trailing_var_arg = true)]
struct Args {
    args: Vec<String>,
}

fn main() {
    // TODO: check if running on Linux
    let args = Args::parse();
    // TODO: set up anyhow
    let mut cmd: std::process::Child = Command::new("strace")
        .args(args.args)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = cmd.stderr.as_mut().unwrap();

    // TODO: optimal buffer size?
    let mut buffer = [0; 512];
    loop {
        // TODO: error handling
        let n = stderr.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        let s = String::from_utf8_lossy(&buffer[0..n]);
        print!("{}", s);
    }

    cmd.wait().unwrap();
}
