use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use anyhow::{anyhow, Result};

pub enum Message {
    Syscall(Syscall),
    ParseError { text: String, message: String }
}

pub struct Syscall {
    pub name: String,
    pub args: Vec<SyscallArg>,
    pub return_value: i64,
}

#[derive(Debug)]
pub enum SyscallArg {
    Null,
    // backslash escapes in `text` are unresolved, i.e. you will see a backslash followed by an 'n'
    // rather than a newline
    Quoted { text: String, truncated: bool },
    Symbol(String),
    FlagSet(Vec<String>),
    Number(i64),
    Product(i64, i64),
    Array(Vec<SyscallArg>),
    Struct(HashMap<String, SyscallArg>),
}

pub fn strace(cmd: &Vec<String>, tx: mpsc::Sender<Message>) -> Result<()> {
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
        let n = reader
            .read_line(&mut line)
            .map_err(|e| anyhow!("unable to read output from strace: {}", e))?;
        if n == 0 {
            break;
        }

        if line.starts_with("+++") {
            continue;
        }

        let msg = match parse_syscall(&line) {
            Ok(s) => Message::Syscall(s),
            Err(e) => Message::ParseError { text: line, message: e.to_string() }
        };
        tx.send(msg).map_err(|e| anyhow!("transmit error: {}", e))?;
    }

    let exit_result = child
        .wait()
        .map_err(|e| anyhow!("failed to wait for strace to terminate: {}", e))?;
    if !exit_result.success() {
        return Err(anyhow!("strace returned a non-zero exit code"));
    }
    Ok(())
}

fn parse_syscall(text: &str) -> Result<Syscall> {
    SyscallParser::new(text).parse()
}

struct SyscallParser<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> SyscallParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            index: 0,
        }
    }

    fn parse(&mut self) -> Result<Syscall> {
        // structure of syscall line:
        //   <syscall name>(<args>...) = <return> <explanation>
        let name = self.consume_name()?;
        self.require('(')?;

        let mut args = Vec::new();
        while let Some(arg) = self.consume_arg()? {
            args.push(arg);
        }
        self.require(')')?;
        self.whitespace();
        self.require('=')?;
        self.whitespace();
        let return_value = self.consume_i64()?;

        Ok(Syscall {
            name,
            args,
            return_value,
        })
    }

    // invariant: consume_XXX is called with self.index on the first character of the token,
    // and returns with self.index on the first character of the next token

    fn consume_name(&mut self) -> Result<String> {
        let start = self.index;
        loop {
            let c = match self.read() {
                Some(c) => c,
                None => break,
            };

            if start == self.index {
                if !c.is_alphabetic() {
                    return Err(anyhow!("expected to see name"));
                }
            } else if !c.is_alphanumeric() && c != '_' {
                break;
            }
            self.advance();
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.index])?.to_string())
    }

    fn consume_arg(&mut self) -> Result<Option<SyscallArg>> {
        // arg can be:
        //   - the literal NULL
        //   - a symbol (e.g., O_RDONLY)
        //   - a flag set (e.g., O_RDONLY|O_CLOEXEC)
        //   - a quoted string (e.g., "path/to/file")
        //     - may be followed by ellipsis
        //   - a number (e.g., 1024)
        //   - a number multipled by another number (e.g., 8192*1024)
        //   - an array (e.g., ["df", "-h"])
        //   - a struct (e.g., {field1=val1, field2=val2 ...})
        //     - the final field of the struct may be followed by an ellipsis
        //   - a C-style comment (e.g., /* 40 vars */)
        //

        // this technically matches malformed strings like "(,a,b)"
        self.skip(',');
        self.whitespace();

        let c = match self.read() {
            Some(c) => c,
            None => return Ok(None),
        };

        if c == ')' {
            Ok(None)
        } else if c.is_ascii_alphabetic() {
            let mut flags = self.consume_flagset()?;
            if flags.len() == 1 {
                Ok(Some(SyscallArg::Symbol(flags.remove(0))))
            } else {
                Ok(Some(SyscallArg::FlagSet(flags)))
            }
        } else if c.is_ascii_digit() {
            let x = self.consume_i64()?;
            Ok(Some(SyscallArg::Number(x)))
        } else if c == '"' {
            let (text, truncated) = self.consume_quoted()?;
            Ok(Some(SyscallArg::Quoted { text, truncated }))
        } else {
            Err(anyhow!("could not parse arg"))
        }
    }

    fn consume_flagset(&mut self) -> Result<Vec<String>> {
        let mut r = Vec::new();
        loop {
            let symbol = self.consume_name()?;
            r.push(symbol);
            if self.read() != Some('|') {
                break;
            } else {
                self.advance();
            }
        }
        Ok(r)
    }

    fn consume_i64(&mut self) -> Result<i64> {
        let radix = self.consume_optional_i64_prefix();
        let mut r = 0i64;
        loop {
            let c = match self.read() {
                Some(c) => c,
                None => break,
            };

            match c.to_digit(radix) {
                Some(v) => {
                    r *= radix as i64;
                    r += v as i64;
                    self.advance();
                }
                None => break,
            }
        }
        Ok(r)
    }

    fn consume_quoted(&mut self) -> Result<(String, bool)> {
        self.require('"')?;
        let start = self.index;
        let end;
        loop {
            let c = self.read_no_eof()?;
            if c == '"' {
                end = self.index;
                self.advance();
                break;
            }
            self.advance();
        }

        let truncated = if self.starts_with("...") {
            self.advance_n(3);
            true
        } else {
            false
        };

        Ok((
            std::str::from_utf8(&self.bytes[start..end])?.to_string(),
            truncated,
        ))
    }

    // returns the radix (e.g., 16 for hexadecimal)
    fn consume_optional_i64_prefix(&mut self) -> u32 {
        if let (Some('0'), Some('x')) = self.read_two() {
            self.advance_n(2);
            16
        } else {
            10
        }
    }

    fn require(&mut self, expected: char) -> Result<()> {
        let actual = self.read_no_eof()?;
        if actual != expected {
            return Err(anyhow!("expected {:?}, got {:?}", expected, actual));
        }
        self.advance();
        Ok(())
    }

    fn require_eof(&mut self) -> Result<()> {
        if !self.done() {
            return Err(anyhow!("expected end of input"));
        }
        Ok(())
    }

    fn whitespace(&mut self) {
        while let Some(c) = self.read() {
            if !c.is_ascii_whitespace() {
                break;
            }
            self.advance();
        }
    }

    fn skip(&mut self, c: char) {
        if self.read() == Some(c) {
            self.advance();
        }
    }

    fn read(&mut self) -> Option<char> {
        if self.done() {
            return None;
        }
        Some(self.bytes[self.index] as char)
    }

    fn read_no_eof(&mut self) -> Result<char> {
        self.read().ok_or(anyhow!("end of file"))
    }

    fn read_two(&mut self) -> (Option<char>, Option<char>) {
        (
            self.bytes.get(self.index).map(|x| *x as char),
            self.bytes.get(self.index + 1).map(|x| *x as char),
        )
    }

    fn starts_with(&mut self, prefix: &str) -> bool {
        (self.bytes.len() - self.index) >= prefix.len()
            && &self.bytes[self.index..(self.index + prefix.len())] == prefix.as_bytes()
    }

    fn advance(&mut self) {
        self.index += 1;
    }

    fn advance_n(&mut self, n: usize) {
        self.index += n;
    }

    fn done(&self) -> bool {
        self.index >= self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::strace::parse_syscall;

    use super::{SyscallArg, SyscallParser};

    #[test]
    fn test_syscall_parse() {
        let mut sc = parse_syscall("close(3) = 0").unwrap();
        assert_eq!(sc.name, "close");
        assert_eq!(sc.args.len(), 1);
        assert_arg_number(&sc.args[0], 3);
        assert_eq!(sc.return_value, 0);

        sc = parse_syscall("openat(AT_FDCWD, \"/proc/self/mountinfo\", O_RDONLY|O_CLOEXEC) = 3")
            .unwrap();
        assert_eq!(sc.name, "openat");
        assert_eq!(sc.args.len(), 3);
        assert_arg_symbol(&sc.args[0], "AT_FDCWD");
        assert_arg_string(&sc.args[1], "/proc/self/mountinfo", false);
        assert_arg_flagset(
            &sc.args[2],
            &vec!["O_RDONLY".to_string(), "O_CLOEXEC".to_string()],
        );
        assert_eq!(sc.return_value, 3);

        sc = parse_syscall("read(3, \"# Locale name alias data base.\\n#\"..., 4096) = 2996")
            .unwrap();
        assert_eq!(sc.name, "read");
        assert_eq!(sc.args.len(), 3);
        assert_arg_number(&sc.args[0], 3);
        assert_arg_string(&sc.args[1], "# Locale name alias data base.\\n#", true);
        assert_arg_number(&sc.args[2], 4096);
        assert_eq!(sc.return_value, 2996);
    }

    #[test]
    fn test_consume_name() {
        let mut p = SyscallParser::new("read");
        let mut s = p.consume_name().unwrap();
        assert_eq!(s, "read");

        p = SyscallParser::new("syscall_whatever(");
        s = p.consume_name().unwrap();
        assert_eq!(s, "syscall_whatever");

        p = SyscallParser::new("123");
        assert!(p.consume_name().is_err());
    }

    #[test]
    fn test_consume_arg() {
        let mut p = SyscallParser::new("O_RDONLY, 123, \"hello\"");
        let mut arg = p.consume_arg().unwrap().unwrap();
        assert_arg_symbol(&arg, "O_RDONLY");

        arg = p.consume_arg().unwrap().unwrap();
        assert_arg_number(&arg, 123);

        arg = p.consume_arg().unwrap().unwrap();
        assert_arg_string(&arg, "hello", false);

        assert!(p.consume_arg().unwrap().is_none());
    }

    #[test]
    fn test_consume_flagset() {
        let mut p = SyscallParser::new("O_RDONLY|O_CLOEXEC");
        let flags = p.consume_flagset().unwrap();
        assert_eq!(flags.len(), 2);
        assert_eq!(flags[0], "O_RDONLY");
        assert_eq!(flags[1], "O_CLOEXEC");
    }

    #[test]
    fn test_consume_i64() {
        let mut p = SyscallParser::new("123");
        let mut v = p.consume_i64().unwrap();
        assert_eq!(v, 123);

        p = SyscallParser::new("0xfF abc");
        v = p.consume_i64().unwrap();
        assert_eq!(v, 0xFF);
    }

    #[test]
    fn test_consume_quoted() {
        let mut p = SyscallParser::new("\"hello\"");
        let (mut s, mut truncated) = p.consume_quoted().unwrap();
        assert_eq!(s, "hello");
        assert!(!truncated);

        p = SyscallParser::new("\"\"...");
        (s, truncated) = p.consume_quoted().unwrap();
        assert_eq!(s, "");
        assert!(p.done());
        assert!(truncated);
    }

    #[test]
    fn test_advance_whitespace() {
        let mut p = SyscallParser::new("   ab    ");
        p.whitespace();
        assert_eq!(p.read().unwrap(), 'a');
        p.advance();
        assert_eq!(p.read().unwrap(), 'b');
        p.advance();
        assert_eq!(p.read().unwrap(), ' ');
        p.whitespace();
        assert!(p.done());
    }

    fn assert_arg_string(arg: &SyscallArg, expected_text: &str, expected_truncated: bool) {
        if let SyscallArg::Quoted { text, truncated } = arg {
            assert_eq!(text, expected_text);
            assert_eq!(*truncated, expected_truncated);
        } else {
            panic!("expected SyscallArg::String, got {:?}", arg);
        }
    }

    fn assert_arg_flagset(arg: &SyscallArg, expected: &Vec<String>) {
        if let SyscallArg::FlagSet(s) = arg {
            assert_eq!(s, expected);
        } else {
            panic!("expected SyscallArg::FlagSet, got {:?}", arg);
        }
    }

    fn assert_arg_symbol(arg: &SyscallArg, expected: &str) {
        if let SyscallArg::Symbol(s) = arg {
            assert_eq!(s, expected);
        } else {
            panic!("expected SyscallArg::Symbol, got {:?}", arg);
        }
    }

    fn assert_arg_number(arg: &SyscallArg, expected: i64) {
        if let SyscallArg::Number(x) = arg {
            assert_eq!(*x, expected);
        } else {
            panic!("expected SyscallArg::Number, got {:?}", arg);
        }
    }
}
