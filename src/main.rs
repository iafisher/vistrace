use std::io::Read;
use std::process::{Command, Stdio};

use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    args: Vec<String>,
}

fn main() {
    // TODO: check if running on Linux
    let args = Args::parse();
    // TODO: set up anyhow
    let cmd: std::process::Child = Command::new("echo")
        .args(args.args)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = cmd.stderr.unwrap();

    // TODO: optimal buffer size?
    let mut buffer = [0; 512];
    loop {
        // TODO: error handling
        let n = stderr.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        println!("read: {:?}", &buffer[0..n]);
    }
}
