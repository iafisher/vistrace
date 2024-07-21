use std::sync::mpsc;
use std::{env, process, thread};

use anyhow::Result;
use clap::Parser;

mod strace;

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

    let (tx, rx) = mpsc::channel::<strace::Message>();

    let handle = thread::spawn(move || strace::strace(&args.args, tx));

    for msg in rx.iter() {
        match msg {
            strace::Message::Syscall(s) => {
                println!("got one: {}", s.name);
            }
            strace::Message::ParseError { text, message } => {
                eprintln!("warning: could not parse strace line: {}", message);
                eprintln!("  ==> {:?}", text);
            }
        }
    }

    // unwrap() because join() returns error only if thread panicked
    // the '?' propagates any actual errors the thread returned
    handle.join().unwrap()?;
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
