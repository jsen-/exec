#![allow(dead_code)]
#![allow(unused_imports)]

extern crate libc;

use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvError, Sender};
use std::thread;
use std::error::Error;
use std::io::{self, Read};
use std::time::Duration;
use std::cell::UnsafeCell;
use std::sync::Arc;
use std::ops::Deref;
use std::mem::transmute;

use std::fmt::Display;

#[cfg(unix)]
#[path = "sys/unix.rs"]
mod wait_timeout;

#[cfg(windows)]
#[path = "sys/windows.rs"]
mod wait_timeout;

use wait_timeout::ExitStatus;

#[derive(Debug)]
pub enum Io<T> {
    Stdout(T),
    Stderr(T),
}

pub enum ProcessData {
    Data(Io<Box<[u8]>>),
    Exit(ExitStatus),
}

#[derive(Debug)]
pub enum ProcessError {
    Io(Io<io::Error>),
    // Recv(RecvError),
    Exit(io::Error),
    Timeout,
}

impl ::std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match *self {
            ProcessError::Io(Io::Stdout(ref _err)) => write!(f, "stdout error"),
            ProcessError::Io(Io::Stderr(ref _err)) => write!(f, "stderr error"),
            // ProcessError::Recv(ref _err) => write!(f, "recv error"),
            ProcessError::Exit(ref _exit_status) => write!(f, "exit error"),
            ProcessError::Timeout => write!(f, "timeout"),
        }?;

        Ok(())
    }
}

impl Error for ProcessError {
    fn description(&self) -> &str {
        "Process error occurred"
    }
}

type MyResult = Result<ProcessData, ProcessError>;

fn create_reader<R, F>(tx: Sender<MyResult>, mut stream: R, translator: F)
where
    R: Read + Send + 'static,
    F: Fn(Box<[u8]>) -> Io<Box<[u8]>>,
    F: Send + 'static,
{
    thread::spawn(move || {
        const CAPACITY: usize = 4096;
        loop {
            let mut buf = Vec::with_capacity(CAPACITY);
            unsafe {
                buf.set_len(CAPACITY);
                let x = match stream.read(&mut buf) {
                    Ok(n) => {
                        buf.set_len(n);
                        if n == 0 {
                            return;
                        }
                        Ok(ProcessData::Data(translator(buf.into_boxed_slice())))
                    }
                    // TODO: this is not always stdout
                    Err(e) => Err(ProcessError::Io(Io::Stdout(e))),
                };

                if let Err(_) = tx.send(x) {
                    eprintln!("output reader send error");
                    return;
                };
            }
        }
    });
}

pub struct ProcessOutputIterator {
    rx: Receiver<MyResult>,
    exit_status: Option<ExitStatus>,
}

impl ProcessOutputIterator {
    pub fn exit_status(self) -> Option<ExitStatus> {
        self.exit_status
    }
}

impl Iterator for ProcessOutputIterator {
    type Item = Result<Io<Box<[u8]>>, ProcessError>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.rx.recv() {
            Ok(value) => match value {
                Ok(ProcessData::Data(data)) => Some(Ok(data)),
                Ok(ProcessData::Exit(exit_status)) => {
                    self.exit_status = Some(exit_status);
                    None
                },
                Err(err) => Some(Err(err)),
            }
            Err(_) => None//Some(Err(ProcessError::Recv(e))),
        }
    }
}

trait ProcessOutput {
    fn iter<T>(self, timeout: T) -> ProcessOutputIterator
    where
        T: Into<Option<Duration>> + Send + 'static;
}

impl ProcessOutput for ::std::process::Child {
    fn iter<T>(mut self, timeout: T) -> ProcessOutputIterator
    where
        T: Send + 'static,
        T: Into<Option<Duration>>,
    {
        let (tx, rx) = channel::<MyResult>();
        let pid = wait_timeout::get_id(&self);

        if let Some(stdout) = self.stdout.take() {
            create_reader(tx.clone(), stdout, Io::Stdout);
        }
        if let Some(stderr) = self.stderr.take() {
            create_reader(tx.clone(), stderr, Io::Stderr);
        }

        let timeout_tx = tx.clone();
        thread::spawn(move || {
            let res = if let Some(timeout) = timeout.into() {
                wait_timeout::wait_timeout(pid, timeout)
            } else {
                wait_timeout::wait(pid).map(Option::Some)
            };
            let send = match res {
                Ok(None) => Err(ProcessError::Timeout),
                Ok(Some(exit_status)) => Ok(ProcessData::Exit(exit_status)),
                Err(e) => Err(ProcessError::Exit(e)),
            };
            if let Err(_) = timeout_tx.send(send) {
                eprintln!("timeout transmit error");
                return;
            };
        });

        ProcessOutputIterator {
            rx: rx,
            exit_status: None,
        }
    }
}

fn print(bytes: &[u8]) {
    use std::io::Write;
    let stdout = std::io::stdout();
    stdout.lock().write_all(bytes).unwrap();
}

fn real_main() -> Result<ExitStatus, Box<Error>> {
    let mut command = Command::new("/bin/sh");
    command.arg("test.sh");
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut process = command.spawn()?.iter(Duration::from_secs(2));

    for data in process.by_ref() {
        match data? {
            Io::Stdout(data) => print(&data),
            Io::Stderr(_) => {}
        }
    }

    Ok(process
        .exit_status()
        .expect("exit status not available after iterator finished"))
}

fn main() {
    match real_main() {
        Ok(x) => eprintln!("{:?}", x),
        Err(err) => {
            eprintln!("Error: {:?}", err);
            std::process::exit(1);
        }
    }
}
