#![deny(missing_docs)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![doc(html_root_url = "https://docs.rs/ghci/0.1.0")]

//! A crate to manage and communicate with `ghci` sessions
//!
//! ```
//! # use ghci::Ghci;
//! let mut ghci = Ghci::new().unwrap();
//! let out = ghci.eval("putStrLn \"Hello world\"").unwrap();
//! assert_eq!(&out.stdout, "Hello world\n");
//! ```
//!
//! See [`Ghci`] documentation for more examples

use core::time::Duration;
use nix::poll::{poll, PollFd, PollFlags};
use nonblock::NonBlockingReader;
use std::io::{ErrorKind, LineWriter, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};

/// A ghci session handle
///
/// The session is stateful, so the order of interaction matters
pub struct Ghci {
    /// `ghci` process
    child: Child,
    /// Buffered child stdin writer
    stdin: LineWriter<ChildStdin>,
    /// Non-blocking child stdout reader
    stdout: NonBlockingReader<ChildStdout>,
    /// Raw fd for child stdout used to wait for events
    stdout_fd: RawFd,
    /// Non-blocking child stderr reader
    stderr: NonBlockingReader<ChildStderr>,
    /// Raw fd for child stderr used to wait for events
    stderr_fd: RawFd,
    /// Current timeout value
    timeout: Option<Duration>,
}

#[derive(Debug)]
#[non_exhaustive]
/// Result for a ghci evaluation
pub struct EvalOutput {
    /// stdout for the result of the ghci evaluation
    pub stdout: String,
    /// stderr for the result of the ghci evaluation
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
/// Errors associated with a [`Ghci`] session
pub enum GhciError {
    /// The evaluation timed out
    ///
    /// Note: The Ghci session is not in a good state and needs to be killed
    #[error("ghci session timed out waiting on output")]
    Timeout,
    /// IO error from the underlying child process management
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    /// Poll error when waiting on ghci stdout/stderr
    #[error("Poll error: {0}")]
    PollError(#[from] nix::errno::Errno),
}

/// A convenient alias for [`std::result::Result`] using a [`GhciError`]
pub type Result<T> = std::result::Result<T, GhciError>;

// Use a prompt that is unlikely to be part of the stdout of the ghci session
const PROMPT: &str = "__ghci_rust_prompt__>\n";

impl Ghci {
    /// Create a new ghci session
    ///
    /// It will use `ghci` on your `PATH` by default, but can be overridden to use any `ghci` by
    /// setting the `GHCI_PATH` environment variable pointing at the binary to use
    ///
    /// # Errors
    ///
    /// Returns [`IOError`] when it encounters IO errors as part of spawning the `ghci` subprocess
    ///
    /// [`IOError`]: GhciError::IOError
    pub fn new() -> Result<Self> {
        const PIPE_ERR: &str = "pipe should be present";

        let ghci = std::env::var("GHCI_PATH").unwrap_or_else(|_| "ghci".to_string());

        let mut child = Command::new(ghci)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut stdin = LineWriter::new(child.stdin.take().expect(PIPE_ERR));
        let mut stdout = child.stdout.take().expect(PIPE_ERR);
        let stderr = child.stderr.take().expect(PIPE_ERR);

        clear_blocking_reader_until(&mut stdout, b"> ")?;

        // Setup a known prompt/multi-line prompt
        stdin.write_all(b":set prompt \"")?;
        stdin.write_all(PROMPT[..PROMPT.len() - 1].as_bytes())?;
        stdin.write_all(b"\\n\"\n")?;
        clear_blocking_reader_until(&mut stdout, PROMPT.as_bytes())?;

        stdin.write_all(b":set prompt-cont \"\"\n")?;
        clear_blocking_reader_until(&mut stdout, PROMPT.as_bytes())?;

        Ok(Self {
            stdin,
            stdout_fd: stdout.as_raw_fd(),
            stdout: NonBlockingReader::from_fd(stdout)?,
            stderr_fd: stderr.as_raw_fd(),
            stderr: NonBlockingReader::from_fd(stderr)?,
            child,
            timeout: None,
        })
    }

    /// Evaluate/run a statement
    ///
    /// ```
    /// # use ghci::Ghci;
    /// let mut ghci = Ghci::new().unwrap();
    /// let out = ghci.eval("putStrLn \"Hello world\"").unwrap();
    /// assert_eq!(&out.stdout, "Hello world\n");
    /// ```
    ///
    /// Multi-line inputs are also supported. The evaluation output may contain both stdout and
    /// stderr:
    ///
    /// ```
    /// # use ghci::Ghci;
    /// let mut ghci = Ghci::new().unwrap();
    /// ghci.import(&["System.IO"]); // imports not supported as part of multi-line inputs
    ///
    /// let out = ghci.eval(r#"
    /// do
    ///   hPutStrLn stdout "Output on stdout"
    ///   hPutStrLn stderr "Output on stderr"
    ///   hPutStrLn stdout "And a bit more on stdout"
    /// "#).unwrap();
    ///
    /// assert_eq!(&out.stderr, "Output on stderr\n");
    /// assert_eq!(&out.stdout, "Output on stdout\nAnd a bit more on stdout\n");
    /// ```
    ///
    /// # Errors
    ///
    /// - Returns a [`Timeout`] if the evaluation timeout (set by [`Ghci::set_timeout`])
    /// is reached before the evaluation completes.
    /// - Returns a [`IOError`] when encounters an IO error on the `ghci` subprocess
    /// `stdin`, `stdout`, or `stderr`.
    /// - Returns a [`PollError`] when waiting for output, if the `ghci` subprocess
    /// `stdout` or `stderr` is closed (upon a crash for example)
    ///
    /// [`Timeout`]: GhciError::Timeout
    /// [`IOError`]: GhciError::IOError
    /// [`PollError`]: GhciError::PollError
    pub fn eval(&mut self, input: &str) -> Result<EvalOutput> {
        self.stdin.write_all(b":{\n")?;
        self.stdin.write_all(input.as_bytes())?;
        self.stdin.write_all(b"\n:}\n")?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        let timeout = self
            .timeout
            .and_then(|d| d.as_millis().try_into().ok())
            .unwrap_or(-1);

        loop {
            let mut poll_fds = [
                PollFd::new(self.stderr_fd, PollFlags::POLLIN),
                PollFd::new(self.stdout_fd, PollFlags::POLLIN),
            ];

            let ret = poll(&mut poll_fds, timeout)?;

            if ret == 0 {
                return Err(GhciError::Timeout);
            }

            if poll_fds[0].any() == Some(true) {
                self.stderr.read_available_to_string(&mut stderr)?;
            }

            if poll_fds[1].any() == Some(true) {
                self.stdout.read_available_to_string(&mut stdout)?;

                if stdout.ends_with(PROMPT) {
                    stdout.truncate(stdout.len() - PROMPT.len());
                    break;
                }
            }
        }

        Ok(EvalOutput { stdout, stderr })
    }

    /// Set a timeout for evaluations
    ///
    /// ```
    /// # use ghci::{Ghci, GhciError};
    /// # use std::time::Duration;
    /// let mut ghci = Ghci::new().unwrap();
    /// ghci.import(&["Control.Concurrent"]).unwrap();
    ///
    /// let res = ghci.eval("threadDelay 50000");
    /// assert!(matches!(res, Ok(_)));
    ///
    /// ghci.set_timeout(Some(Duration::from_millis(20)));
    ///
    /// let res = ghci.eval("threadDelay 50000");
    /// assert!(matches!(res, Err(GhciError::Timeout)));
    /// ```
    ///
    /// By default, no timeout is set.
    ///
    /// Note: When a [`Timeout`] error is triggered, the `ghci` session **must** be closed with
    /// [`Ghci::close`] or [`Drop`]ed in order to properly stop the corresponding evaluation.
    /// If the evaluation is left to finish after a timeout occurs, the session is then left in a
    /// bad state that is not recoverable.
    ///
    /// [`Timeout`]: GhciError::Timeout
    #[inline]
    pub fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    /// Import multiple modules
    ///
    /// ```
    /// # use ghci::Ghci;
    /// let mut ghci = Ghci::new().unwrap();
    /// ghci.import(&["Data.Char", "Control.Applicative"]).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Same as [`Ghci::eval`]
    #[inline]
    pub fn import(&mut self, modules: &[&str]) -> Result<()> {
        let mut line = String::from(":module ");
        line.push_str(&modules.join(" "));

        self.eval(&line)?;

        Ok(())
    }

    /// Load multiple modules by file path
    ///
    /// # Errors
    ///
    /// Same as [`Ghci::eval`]
    #[inline]
    pub fn load(&mut self, paths: &[&Path]) -> Result<()> {
        let mut line = String::from(":load");

        for path in paths {
            line.push_str(&format!(" {}", path.display()));
        }

        self.eval(&line)?;

        Ok(())
    }

    /// Close the ghci session
    ///
    /// Closing explicitly is not necessary as the [`Drop`] impl will take care of it. This
    /// function does however give the possibility to properly handle errors on close.
    ///
    /// # Errors
    ///
    /// If the underlying child process has already exited a [`IOError`] with
    /// [`InvalidInput`] error is returned
    ///
    /// [`IOError`]: GhciError::IOError
    /// [`InvalidInput`]: std::io::ErrorKind::InvalidInput
    #[inline]
    pub fn close(mut self) -> Result<()> {
        Ok(self.child.kill()?)
    }
}

impl Drop for Ghci {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
        }
    }
}

// Helper function to clear data from a blocking reader until a pattern is seen
// - the pattern is also cleared
// - the pattern has to be at the end of a given read (otherwise it will hang)
// - limited to 1024 bytes
fn clear_blocking_reader_until(mut r: impl Read, expected_end: &[u8]) -> std::io::Result<()> {
    let mut buffer = [0; 1024];
    let mut end = 0;
    loop {
        match r.read(&mut buffer[end..]) {
            Ok(0) => return Ok(()),
            Ok(bytes) => {
                end += bytes;
                if buffer[..end].ends_with(expected_end) {
                    return Ok(());
                }
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error() {
        let mut ghci = Ghci::new().unwrap();
        let res = ghci.eval("x ::").unwrap();
        assert!(res.stderr.contains("parse error"));
    }
}
