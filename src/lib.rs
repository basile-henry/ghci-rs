#![deny(missing_docs)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![doc(html_root_url = "https://docs.rs/ghci/0.2.1")]

//! A crate to manage and communicate with `ghci` sessions
//!
//! ```
//! # use ghci::Ghci;
//! #
//! # fn main() -> ghci::Result<()> {
//! let mut ghci = Ghci::new()?;
//! let out = ghci.eval("1 + 1")?;
//! assert_eq!(out, "2\n");
//! #
//! #   Ok(())
//! # }
//! ```
//!
//! See [`Ghci`] documentation for more examples.
//!
//! # Platform support
//!
//! This crate uses Unix-specific APIs (`nix::poll`, file descriptors) and only supports
//! Unix platforms (Linux, macOS, BSDs).

use core::time::Duration;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use nonblock::NonBlockingReader;
use std::io::{ErrorKind, LineWriter, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

pub mod haskell;
pub use haskell::{FromHaskell, HaskellParseError, ToHaskell};

#[cfg(feature = "derive")]
pub use ghci_derive::{FromHaskell, ToHaskell};

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
    /// Note: The Ghci session is not be in a good state and needs to be closed
    #[error("ghci session timed out waiting on output")]
    Timeout,
    /// IO error from the underlying child process management
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    /// Poll error when waiting on ghci stdout/stderr
    #[error("Poll error: {0}")]
    PollError(#[from] nix::errno::Errno),
    /// The evaluation produced output on stderr (Haskell error)
    #[error("ghci eval error:\n{stderr}")]
    EvalError {
        /// stdout produced before/during the error
        stdout: String,
        /// stderr output (the error message)
        stderr: String,
    },
    /// Error parsing Haskell expressions
    #[error("Haskell parse error: {0}")]
    HaskellParse(#[from] haskell::HaskellParseError),
    /// Input attempted to change the ghci prompt, which would break the session
    ///
    /// Note: this is a best-effort check for direct `:set prompt` commands. Indirect
    /// changes (e.g. via `:cmd`) are not detected and will break the session.
    #[error("disallowed input: {0}")]
    DisallowedInput(&'static str),
}

/// A convenient alias for [`std::result::Result`] using a [`GhciError`]
pub type Result<T> = std::result::Result<T, GhciError>;

// Use a prompt that is unlikely to be part of the stdout of the ghci session
const PROMPT: &str = "__ghci_rust_prompt__>\n";

/// Builder for configuring and creating [`Ghci`] sessions
///
/// ```
/// # use ghci::GhciBuilder;
/// #
/// # fn main() -> ghci::Result<()> {
/// let mut ghci = GhciBuilder::new()
///     .arg("-XOverloadedStrings")
///     .build()?;
/// let out = ghci.eval("1 + 1")?;
/// assert_eq!(out, "2\n");
/// #
/// #   Ok(())
/// # }
/// ```
pub struct GhciBuilder {
    ghci_path: Option<String>,
    args: Vec<String>,
    working_dir: Option<PathBuf>,
}

impl Default for GhciBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GhciBuilder {
    /// Create a new builder with default settings
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ghci_path: None,
            args: Vec::new(),
            working_dir: None,
        }
    }

    /// Set the path to the ghci binary
    ///
    /// Overrides the `GHCI_PATH` environment variable. If neither is set, `"ghci"` is used.
    #[must_use]
    pub fn ghci_path(mut self, path: impl Into<String>) -> Self {
        self.ghci_path = Some(path.into());
        self
    }

    /// Add a single argument to pass to ghci
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments to pass to ghci
    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set the working directory for the ghci process
    #[must_use]
    pub fn working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(path.into());
        self
    }

    /// Build and start the ghci session
    ///
    /// # Errors
    ///
    /// Returns [`IOError`] when it encounters IO errors as part of spawning the `ghci` subprocess
    ///
    /// # Panics
    ///
    /// Panics if the child process stdin, stdout, or stderr pipes are unexpectedly missing.
    ///
    /// [`IOError`]: GhciError::IOError
    pub fn build(self) -> Result<Ghci> {
        const PIPE_ERR: &str = "pipe should be present";

        let ghci_path = self
            .ghci_path
            .or_else(|| std::env::var("GHCI_PATH").ok())
            .unwrap_or_else(|| "ghci".to_string());

        let mut cmd = Command::new(ghci_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Always ignore the user's ~/.ghci to ensure the default prompt "> " is present.
        // Users who need custom .ghci logic can pass their own script via
        // `.arg("-ghci-script=/path/to/script")`.
        cmd.arg("-ignore-dot-ghci");
        if !self.args.is_empty() {
            cmd.args(&self.args);
        }

        if let Some(dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        let mut child = cmd.spawn()?;

        let mut stdin = LineWriter::new(child.stdin.take().expect(PIPE_ERR));
        let mut stdout = child.stdout.take().expect(PIPE_ERR);
        let stderr = child.stderr.take().expect(PIPE_ERR);

        clear_blocking_reader_until(&mut stdout, b"> ")?;

        // Setup a known prompt/multi-line prompt
        stdin.write_all(b":set prompt \"")?;
        stdin.write_all(&PROMPT.as_bytes()[..PROMPT.len() - 1])?;
        stdin.write_all(b"\\n\"\n")?;
        clear_blocking_reader_until(&mut stdout, PROMPT.as_bytes())?;

        stdin.write_all(b":set prompt-cont \"\"\n")?;
        clear_blocking_reader_until(&mut stdout, PROMPT.as_bytes())?;

        Ok(Ghci {
            stdin,
            stdout_fd: stdout.as_raw_fd(),
            stdout: NonBlockingReader::from_fd(stdout)?,
            stderr_fd: stderr.as_raw_fd(),
            stderr: NonBlockingReader::from_fd(stderr)?,
            child,
            timeout: None,
        })
    }
}

impl Ghci {
    /// Create a new ghci session
    ///
    /// It will use `ghci` on your `PATH` by default, but can be overridden to use any `ghci` by
    /// setting the `GHCI_PATH` environment variable pointing at the binary to use.
    ///
    /// For more configuration options, see [`GhciBuilder`].
    ///
    /// # Errors
    ///
    /// Returns [`IOError`] when it encounters IO errors as part of spawning the `ghci` subprocess
    ///
    /// [`IOError`]: GhciError::IOError
    pub fn new() -> Result<Self> {
        GhciBuilder::new().build()
    }

    /// Evaluate/run a statement
    ///
    /// Returns only the stdout output. If ghci produces any stderr output (indicating an error),
    /// an [`EvalError`] is returned instead.
    ///
    /// For cases where stderr output is expected and should not be treated as an error,
    /// use [`eval_raw`] instead.
    ///
    /// ```
    /// # use ghci::Ghci;
    /// #
    /// # fn main() -> ghci::Result<()> {
    /// let mut ghci = Ghci::new()?;
    /// let out = ghci.eval("putStrLn \"Hello world\"")?;
    /// assert_eq!(out, "Hello world\n");
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    ///
    /// Haskell errors are surfaced as Rust errors:
    ///
    /// ```
    /// # use ghci::{Ghci, GhciError};
    /// #
    /// # fn main() -> ghci::Result<()> {
    /// let mut ghci = Ghci::new()?;
    /// let res = ghci.eval("x ::");
    /// assert!(matches!(res, Err(GhciError::EvalError { .. })));
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - Returns an [`EvalError`] if ghci produces output on stderr.
    /// - Returns a [`Timeout`] if the evaluation timeout (set by [`Ghci::set_timeout`])
    ///   is reached before the evaluation completes.
    /// - Returns a [`IOError`] when encounters an IO error on the `ghci` subprocess
    ///   `stdin`, `stdout`, or `stderr`.
    /// - Returns a [`PollError`] when waiting for output, if the `ghci` subprocess
    ///   `stdout` or `stderr` is closed (upon a crash for example)
    ///
    /// [`EvalError`]: GhciError::EvalError
    /// [`Timeout`]: GhciError::Timeout
    /// [`IOError`]: GhciError::IOError
    /// [`PollError`]: GhciError::PollError
    /// [`eval_raw`]: Ghci::eval_raw
    pub fn eval(&mut self, input: &str) -> Result<String> {
        let out = self.eval_raw(input)?;

        if out.stderr.is_empty() {
            Ok(out.stdout)
        } else {
            Err(GhciError::EvalError {
                stdout: out.stdout,
                stderr: out.stderr,
            })
        }
    }

    /// Evaluate an expression and parse the result as a Rust value
    ///
    /// This is a convenience method that calls [`Ghci::eval`] and then parses the output
    /// using the [`FromHaskell`] trait.
    ///
    /// ```
    /// # use ghci::Ghci;
    /// #
    /// # fn main() -> ghci::Result<()> {
    /// let mut ghci = Ghci::new()?;
    /// let x: i32 = ghci.eval_as("1 + 1")?;
    /// assert_eq!(x, 2);
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - Returns a [`HaskellParse`] error if the output cannot be parsed as the target type
    /// - Same errors as [`Ghci::eval`]
    ///
    /// [`HaskellParse`]: GhciError::HaskellParse
    pub fn eval_as<T: FromHaskell>(&mut self, input: &str) -> Result<T> {
        let output = self.eval(input)?;
        Ok(T::from_haskell(output.trim_end_matches('\n'))?)
    }

    /// Evaluate/run a statement, returning both stdout and stderr
    ///
    /// Unlike [`eval`], this method does not treat stderr output as an error. This is useful
    /// when you expect output on stderr (e.g. GHC warnings, debug output via `hPutStrLn stderr`).
    ///
    /// ```
    /// # use ghci::Ghci;
    /// #
    /// # fn main() -> ghci::Result<()> {
    /// let mut ghci = Ghci::new()?;
    /// ghci.import(&["System.IO"])?;
    ///
    /// let out = ghci.eval_raw(r#"
    /// do
    ///   hPutStrLn stdout "Output on stdout"
    ///   hPutStrLn stderr "Output on stderr"
    ///   hPutStrLn stdout "And a bit more on stdout"
    /// "#)?;
    ///
    /// assert_eq!(&out.stderr, "Output on stderr\n");
    /// assert_eq!(&out.stdout, "Output on stdout\nAnd a bit more on stdout\n");
    /// #
    /// #   Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - Returns a [`Timeout`] if the evaluation timeout (set by [`Ghci::set_timeout`])
    ///   is reached before the evaluation completes.
    /// - Returns a [`IOError`] when encounters an IO error on the `ghci` subprocess
    ///   `stdin`, `stdout`, or `stderr`.
    /// - Returns a [`PollError`] when waiting for output, if the `ghci` subprocess
    ///   `stdout` or `stderr` is closed (upon a crash for example)
    ///
    /// [`eval`]: Ghci::eval
    /// [`Timeout`]: GhciError::Timeout
    /// [`IOError`]: GhciError::IOError
    /// [`PollError`]: GhciError::PollError
    pub fn eval_raw(&mut self, input: &str) -> Result<EvalOutput> {
        if input.trim_start().starts_with(":set prompt") {
            return Err(GhciError::DisallowedInput(
                ":set prompt and :set prompt-cont are managed by ghci-rs and cannot be changed",
            ));
        }

        self.stdin.write_all(b":{\n")?;
        self.stdin.write_all(input.as_bytes())?;
        self.stdin.write_all(b"\n:}\n")?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        let deadline = self.timeout.map(|d| Instant::now() + d);

        loop {
            let stderr_fd = unsafe { BorrowedFd::borrow_raw(self.stderr_fd) };
            let stdout_fd = unsafe { BorrowedFd::borrow_raw(self.stdout_fd) };
            let mut poll_fds = [
                PollFd::new(stderr_fd, PollFlags::POLLIN),
                PollFd::new(stdout_fd, PollFlags::POLLIN),
            ];

            let poll_timeout = match deadline {
                None => PollTimeout::NONE,
                Some(dl) => {
                    let remaining = dl.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(GhciError::Timeout);
                    }
                    remaining
                        .as_millis()
                        .try_into()
                        .ok()
                        .and_then(|ms: i32| PollTimeout::try_from(ms).ok())
                        .unwrap_or(PollTimeout::NONE)
                }
            };

            let ret = poll(&mut poll_fds, poll_timeout)?;

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
    /// #
    /// # fn main() -> ghci::Result<()> {
    /// let mut ghci = Ghci::new()?;
    /// ghci.import(&["Control.Concurrent"])?;
    ///
    /// let res = ghci.eval("threadDelay 50000");
    /// assert!(matches!(res, Ok(_)));
    ///
    /// ghci.set_timeout(Some(Duration::from_millis(20)));
    ///
    /// let res = ghci.eval("threadDelay 50000");
    /// assert!(matches!(res, Err(GhciError::Timeout)));
    /// #
    /// #   Ok(())
    /// # }
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
    pub const fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    /// Import multiple modules
    ///
    /// ```
    /// # use ghci::Ghci;
    /// #
    /// # fn main() -> ghci::Result<()> {
    /// let mut ghci = Ghci::new()?;
    /// ghci.import(&["Data.Char", "Control.Applicative"])?;
    /// #
    /// #   Ok(())
    /// # }
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
            use std::fmt::Write as _;
            let _ = write!(line, " {}", path.display());
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
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
    }
}

/// A shared ghci session for use across threads (e.g. in tests)
///
/// Wraps a [`Ghci`] session in a `OnceLock<Mutex<...>>` so it can be stored in a `static`
/// and lazily initialized on first use.
///
/// ```
/// # use ghci::{Ghci, SharedGhci};
/// #
/// static GHCI: SharedGhci = SharedGhci::new(|| {
///     let mut ghci = Ghci::new()?;
///     ghci.import(&["Data.Char"])?;
///     Ok(ghci)
/// });
///
/// # fn main() {
/// let mut ghci = GHCI.lock();
/// let out = ghci.eval("ord 'A'").unwrap();
/// assert_eq!(out, "65\n");
/// # }
/// ```
pub struct SharedGhci {
    inner: OnceLock<Mutex<Ghci>>,
    init: fn() -> Result<Ghci>,
}

impl SharedGhci {
    /// Create a new `SharedGhci` with the given initialization function
    ///
    /// The initialization function will be called at most once, on the first call to [`lock`].
    ///
    /// [`lock`]: SharedGhci::lock
    #[must_use]
    pub const fn new(init: fn() -> Result<Ghci>) -> Self {
        Self {
            inner: OnceLock::new(),
            init,
        }
    }

    /// Lock and return a guard to the shared ghci session
    ///
    /// Initializes the session on first call.
    ///
    /// # Panics
    ///
    /// Panics if the initialization function returns an error or the mutex is poisoned.
    pub fn lock(&self) -> MutexGuard<'_, Ghci> {
        self.try_lock()
            .expect("SharedGhci initialization or lock failed")
    }

    /// Try to lock and return a guard to the shared ghci session
    ///
    /// Initializes the session on first call.
    ///
    /// # Panics
    ///
    /// Panics if the initialization function returns an error.
    ///
    /// # Errors
    ///
    /// Returns an [`IOError`] if the mutex is poisoned.
    ///
    /// [`IOError`]: GhciError::IOError
    pub fn try_lock(&self) -> Result<MutexGuard<'_, Ghci>> {
        let mutex = self.inner.get_or_init(|| {
            let ghci = (self.init)().expect("SharedGhci initialization failed");
            Mutex::new(ghci)
        });
        mutex
            .lock()
            .map_err(|e| GhciError::IOError(std::io::Error::other(e.to_string())))
    }
}

// Helper function to clear data from a blocking reader until a pattern is seen
// - the pattern is also cleared
// - the pattern has to be at the end of a given read (otherwise it will hang)
fn clear_blocking_reader_until(mut r: impl Read, expected_end: &[u8]) -> std::io::Result<()> {
    let pat_len = expected_end.len();
    assert!(pat_len < 1024, "pattern must be shorter than the read buffer");
    let mut buffer = [0u8; 1024];
    let mut start = 0; // how many bytes at the front are carried over from the previous read
    loop {
        match r.read(&mut buffer[start..]) {
            Ok(0) => return Ok(()),
            Ok(bytes) => {
                let end = start + bytes;
                if buffer[..end].ends_with(expected_end) {
                    return Ok(());
                }
                // Carry over the last pat_len bytes (or fewer if we don't have enough yet)
                // to the front so the next read appends after them.
                let keep = pat_len.min(end);
                buffer.copy_within(end - keep..end, 0);
                start = keep;
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
        let res = ghci.eval("x ::");
        match res {
            Err(GhciError::EvalError { stderr, .. }) => {
                assert!(stderr.contains("parse error"));
            }
            other => panic!("expected EvalError, got {other:?}"),
        }
    }

    #[test]
    fn parse_error_raw() {
        let mut ghci = Ghci::new().unwrap();
        let res = ghci.eval_raw("x ::").unwrap();
        assert!(res.stderr.contains("parse error"));
    }

    #[test]
    fn eval_as_integer() -> Result<()> {
        let mut ghci = Ghci::new()?;
        let x: i32 = ghci.eval_as("1 + 1")?;
        assert_eq!(x, 2);
        Ok(())
    }

    #[test]
    fn eval_as_boolean() -> Result<()> {
        let mut ghci = Ghci::new()?;
        let b: bool = ghci.eval_as("True")?;
        assert!(b);
        Ok(())
    }

    #[test]
    fn eval_as_string() -> Result<()> {
        let mut ghci = Ghci::new()?;
        let s: String = ghci.eval_as(r#""hello" ++ " world""#)?;
        assert_eq!(s, "hello world");
        Ok(())
    }

    #[test]
    fn eval_as_option() -> Result<()> {
        let mut ghci = Ghci::new()?;
        let opt: Option<i32> = ghci.eval_as("(Just 42)")?;
        assert_eq!(opt, Some(42));
        Ok(())
    }

    #[test]
    fn eval_as_vec() -> Result<()> {
        let mut ghci = Ghci::new()?;
        let vec: Vec<i32> = ghci.eval_as("[1, 2, 3]")?;
        assert_eq!(vec, vec![1, 2, 3]);
        Ok(())
    }

    #[test]
    fn disallow_set_prompt() {
        let mut ghci = Ghci::new().unwrap();
        let res = ghci.eval(":set prompt \"foo> \"");
        assert!(
            matches!(res, Err(GhciError::DisallowedInput(_))),
            "expected DisallowedInput, got {res:?}"
        );
    }

    #[test]
    fn timeout_on_infinite_output() {
        let mut ghci = Ghci::new().unwrap();
        ghci.set_timeout(Some(Duration::from_millis(200)));
        // mapM_ keeps producing output forever — the total-duration timeout must fire
        let res = ghci.eval("mapM_ print [1..]");
        assert!(
            matches!(res, Err(GhciError::Timeout)),
            "expected Timeout, got {res:?}"
        );
    }
}
