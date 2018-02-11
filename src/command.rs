use std::process::Stdio;
use std::ffi::{OsString, OsStr};
use std::collections::HashMap;

pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    env: Option<HashMap<OsString, OsString>>,
    cwd: Option<OsString>,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

impl Command {
    pub fn new<S: AsRef<OsStr>>(program: S) -> Command {
        Command {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            env: None,
            cwd: None,
            flags: 0,
            detach: false,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }
}
