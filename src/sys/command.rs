//! Bounded, typed command execution for local account tools.

use super::{Gecos, GroupName, PasswordRecord, SecretString, ShellPath, UserName};
use crate::error::{CoreError, CoreResult};
use std::{
    io::{self, Read, Write},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

/// The only executables the trusted account boundary may invoke.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KnownProgram {
    Sudo,
    UserAdd,
    UserDel,
    UserMod,
    GroupAdd,
    GroupDel,
    GroupMod,
    GPasswd,
    ChPasswd,
    ChAge,
}

impl KnownProgram {
    /// Stable program name used for process execution and safe diagnostics.
    pub const fn executable(self) -> &'static str {
        match self {
            Self::Sudo => "sudo",
            Self::UserAdd => "useradd",
            Self::UserDel => "userdel",
            Self::UserMod => "usermod",
            Self::GroupAdd => "groupadd",
            Self::GroupDel => "groupdel",
            Self::GroupMod => "groupmod",
            Self::GPasswd => "gpasswd",
            Self::ChPasswd => "chpasswd",
            Self::ChAge => "chage",
        }
    }
}

/// Limits applied to every child process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLimits {
    pub timeout: Duration,
    pub output_bytes: usize,
}

impl Default for CommandLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
            output_bytes: 64 * 1024,
        }
    }
}

/// Safe, non-secret command metadata for previews and test assertions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPreview {
    pub program: KnownProgram,
    pub arguments: Vec<String>,
    pub stdin: Option<&'static str>,
}

impl CommandPreview {
    /// A redacted shell-free representation of the intended invocation.
    pub fn render(&self) -> String {
        let mut rendered = self.program.executable().to_owned();
        for argument in &self.arguments {
            rendered.push(' ');
            rendered.push_str(argument);
        }
        if let Some(stdin) = self.stdin {
            rendered.push_str(" < ");
            rendered.push_str(stdin);
        }
        rendered
    }
}

enum CommandInput {
    PasswordRecord(PasswordRecord),
}

/// A validated command.  It cannot be constructed with an arbitrary program,
/// shell fragment, or raw password-bearing argument.
pub struct CommandSpec {
    program: KnownProgram,
    arguments: Vec<String>,
    input: Option<CommandInput>,
    limits: CommandLimits,
}

impl CommandSpec {
    /// Start a command for a fixed account-management executable.
    pub fn new(program: KnownProgram) -> Self {
        Self {
            program,
            arguments: Vec::new(),
            input: None,
            limits: CommandLimits::default(),
        }
    }

    /// Append a reviewed literal required by the closed tool contracts.
    pub fn fixed_arg(mut self, argument: &'static str) -> CoreResult<Self> {
        if !matches!(
            argument,
            "-a" | "-c" | "-d" | "-l" | "-m" | "-n" | "-r" | "-s" | "0"
        ) {
            return Err(CoreError::Validation {
                field: "command argument",
                reason: "is not part of a reviewed account-tool contract",
            });
        }
        self.arguments.push(argument.to_owned());
        Ok(self)
    }

    /// Append a validated user name.
    pub fn user_name(mut self, username: &UserName) -> Self {
        self.arguments.push(username.as_str().to_owned());
        self
    }

    /// Append a validated group name.
    pub fn group_name(mut self, groupname: &GroupName) -> Self {
        self.arguments.push(groupname.as_str().to_owned());
        self
    }

    /// Append a validated absolute shell path.
    pub fn shell_path(mut self, shell: &ShellPath) -> Self {
        self.arguments.push(shell.as_str().to_owned());
        self
    }

    /// Append a validated GECOS field.
    pub fn gecos(mut self, gecos: &Gecos) -> Self {
        self.arguments.push(gecos.as_str().to_owned());
        self
    }

    /// Attach the sole supported sensitive stdin protocol.
    pub fn password_record(mut self, record: PasswordRecord) -> CoreResult<Self> {
        if self.program != KnownProgram::ChPasswd || !self.arguments.is_empty() {
            return Err(CoreError::Validation {
                field: "chpasswd command",
                reason: "password records require chpasswd with no arguments",
            });
        }
        self.input = Some(CommandInput::PasswordRecord(record));
        Ok(self)
    }

    /// Override bounded execution limits for an operation-specific contract.
    pub fn with_limits(mut self, limits: CommandLimits) -> CoreResult<Self> {
        if limits.timeout.is_zero() || limits.output_bytes == 0 {
            return Err(CoreError::Validation {
                field: "command limits",
                reason: "timeout and output limit must be non-zero",
            });
        }
        self.limits = limits;
        Ok(self)
    }

    /// The fixed executable selected by the operation.
    pub const fn program(&self) -> KnownProgram {
        self.program
    }

    /// Bound execution controls.
    pub const fn limits(&self) -> CommandLimits {
        self.limits
    }

    /// A safe preview which never contains a password record.
    pub fn redacted_preview(&self) -> CommandPreview {
        CommandPreview {
            program: self.program,
            arguments: self.arguments.clone(),
            stdin: self.input.as_ref().map(|_| "<redacted password record>"),
        }
    }
}

/// Non-secret proof that the runner has either direct privilege or a validated
/// sudo timestamp.  It contains no credential and can be cached only for the
/// duration of one operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElevationGrant {
    Direct,
    SudoTimestamp,
}

/// Bounded child output.  This type intentionally does not implement `Debug`
/// because command output can be sensitive in distribution-specific failures.
pub struct CommandResult {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl CommandResult {
    /// Construct a result for deterministic runners.
    pub fn new(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
        Self {
            status,
            stdout,
            stderr,
        }
    }

    /// Process exit status.
    pub fn status(&self) -> &ExitStatus {
        &self.status
    }

    /// Retained stdout, limited by the command contract.
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    /// Retained stderr, limited by the command contract.
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }
}

/// Injectable command/elevation boundary.
pub trait CommandRunner: Send + Sync {
    /// Validate elevation with a one-shot secret and immediately discard it.
    fn authenticate(&self, secret: SecretString) -> CoreResult<ElevationGrant>;

    /// Execute a validated command with a previously obtained grant.
    fn run(&self, grant: ElevationGrant, spec: &CommandSpec) -> CoreResult<CommandResult>;
}

/// Production runner for the Linux-local standard account-tool contracts.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalCommandRunner;

impl CommandRunner for LocalCommandRunner {
    fn authenticate(&self, secret: SecretString) -> CoreResult<ElevationGrant> {
        let limits = CommandLimits::default();
        let result = run_child(
            KnownProgram::Sudo,
            &["-S", "-p", "", "-v"],
            Some(ChildInput::SecretLine(&secret)),
            limits,
        )?;
        if result.status.success() {
            Ok(ElevationGrant::SudoTimestamp)
        } else {
            Err(CoreError::AuthenticationDenied)
        }
    }

    fn run(&self, grant: ElevationGrant, spec: &CommandSpec) -> CoreResult<CommandResult> {
        let mut arguments: Vec<&str> = Vec::new();
        let program = match grant {
            ElevationGrant::Direct => spec.program,
            ElevationGrant::SudoTimestamp => {
                arguments.extend(["-n", "--", spec.program.executable()]);
                KnownProgram::Sudo
            }
        };
        arguments.extend(spec.arguments.iter().map(String::as_str));
        let input = spec.input.as_ref().map(|input| match input {
            CommandInput::PasswordRecord(record) => ChildInput::PasswordRecord(record),
        });
        let result = run_child(program, &arguments, input, spec.limits)?;
        if result.status.success() {
            return Ok(result);
        }
        if grant == ElevationGrant::SudoTimestamp && sudo_capability_failure(result.stderr()) {
            return Err(CoreError::AuthenticationCapability);
        }
        Err(CoreError::ExitStatus {
            program: spec.program.executable(),
            code: result.status.code(),
        })
    }
}

enum ChildInput<'a> {
    SecretLine(&'a SecretString),
    PasswordRecord(&'a PasswordRecord),
}

fn run_child(
    program: KnownProgram,
    arguments: &[&str],
    input: Option<ChildInput<'_>>,
    limits: CommandLimits,
) -> CoreResult<CommandResult> {
    let mut command = Command::new(program.executable());
    command
        .args(arguments)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| command_io_error(program, "spawn", &error))?;

    if let Some(input) = input {
        let write_result = write_input(&mut child, input);
        if let Err(error) = write_result {
            return kill_and_reap(child, program, "stdin write", error);
        }
    }

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            return terminate_and_reap(
                &mut child,
                program,
                CoreError::Io {
                    operation: "stdout pipe",
                    kind: io::ErrorKind::BrokenPipe,
                },
            );
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return terminate_and_reap(
                &mut child,
                program,
                CoreError::Io {
                    operation: "stderr pipe",
                    kind: io::ErrorKind::BrokenPipe,
                },
            );
        }
    };
    let stdout_reader = read_bounded(stdout, limits.output_bytes);
    let stderr_reader = read_bounded(stderr, limits.output_bytes);
    // Every post-spawn path reaches checked wait/kill/reap before both reader
    // joins. Readers are joined even when wait/timeout itself fails.
    let wait = wait_with_timeout(&mut child, program, limits.timeout);
    let stdout = join_reader(stdout_reader, program);
    let stderr = join_reader(stderr_reader, program);
    let wait = wait?;
    let stdout = stdout?;
    let stderr = stderr?;
    if stdout.exceeded || stderr.exceeded {
        return Err(CoreError::OutputLimit {
            program: program.executable(),
            limit: limits.output_bytes,
        });
    }
    let WaitOutcome::Exited(status) = wait;
    Ok(CommandResult::new(status, stdout.bytes, stderr.bytes))
}

fn write_input(child: &mut Child, input: ChildInput<'_>) -> io::Result<()> {
    let mut stdin = child.stdin.take().ok_or_else(|| {
        io::Error::new(io::ErrorKind::BrokenPipe, "child stdin was not available")
    })?;
    match input {
        ChildInput::SecretLine(secret) => {
            secret.write_to(&mut stdin)?;
            stdin.write_all(b"\n")?;
        }
        ChildInput::PasswordRecord(record) => record.write_to(&mut stdin)?,
    }
    stdin.flush()
}

struct BoundedOutput {
    bytes: Vec<u8>,
    exceeded: bool,
}

fn read_bounded<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
) -> thread::JoinHandle<io::Result<BoundedOutput>> {
    thread::spawn(move || {
        let mut retained = Vec::with_capacity(limit.min(8192));
        let mut exceeded = false;
        let mut buffer = [0_u8; 8192];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let available = limit.saturating_sub(retained.len());
            if count > available {
                retained.extend_from_slice(&buffer[..available]);
                exceeded = true;
            } else {
                retained.extend_from_slice(&buffer[..count]);
            }
        }
        Ok(BoundedOutput {
            bytes: retained,
            exceeded,
        })
    })
}

fn join_reader(
    reader: thread::JoinHandle<io::Result<BoundedOutput>>,
    program: KnownProgram,
) -> CoreResult<BoundedOutput> {
    reader
        .join()
        .map_err(|_| CoreError::Io {
            operation: "output reader",
            kind: io::ErrorKind::Other,
        })?
        .map_err(|error| command_io_error(program, "output read", &error))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitOutcome {
    Exited(ExitStatus),
}

fn wait_with_timeout(
    child: &mut Child,
    program: KnownProgram,
    timeout: Duration,
) -> CoreResult<WaitOutcome> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => match child.wait() {
                Ok(status) => return Ok(WaitOutcome::Exited(status)),
                Err(error) => {
                    return terminate_and_reap(
                        child,
                        program,
                        command_io_error(program, "reap", &error),
                    );
                }
            },
            Ok(None) => {}
            Err(error) => {
                return terminate_and_reap(
                    child,
                    program,
                    command_io_error(program, "wait", &error),
                );
            }
        }
        if start.elapsed() >= timeout {
            return terminate_and_reap(
                child,
                program,
                CoreError::Timeout {
                    program: program.executable(),
                    timeout,
                },
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Attempt kill and reap independently, even if the first cleanup operation
/// fails. `Child` has no fallible stdin-close API; input is flushed then dropped
/// before this function is called. Cleanup failure is preserved in the typed
/// partial-completion detail without discarding the original classified error.
fn terminate_and_reap<T>(
    child: &mut Child,
    _program: KnownProgram,
    primary: CoreError,
) -> CoreResult<T> {
    let kill = child.kill();
    let reap = child.wait();
    match (kill, reap) {
        (Ok(()), Ok(_)) => Err(primary),
        (kill, reap) => {
            let detail = match (kill.err(), reap.err()) {
                (Some(kill), Some(reap)) => format!(
                    "{primary}; child cleanup kill={} reap={}",
                    kill.kind(),
                    reap.kind()
                ),
                (Some(kill), None) => format!("{primary}; child cleanup kill={}", kill.kind()),
                (None, Some(reap)) => format!("{primary}; child cleanup reap={}", reap.kind()),
                (None, None) => unreachable!("successful cleanup handled above"),
            };
            Err(CoreError::PartialCompletion { step: detail })
        }
    }
}

fn kill_and_reap<T>(
    mut child: Child,
    program: KnownProgram,
    operation: &'static str,
    error: io::Error,
) -> CoreResult<T> {
    terminate_and_reap(
        &mut child,
        program,
        command_io_error(program, operation, &error),
    )
}

fn command_io_error(
    program: KnownProgram,
    operation: &'static str,
    error: &io::Error,
) -> CoreError {
    if operation == "spawn" && error.kind() == io::ErrorKind::NotFound {
        CoreError::MissingExecutable {
            program: program.executable(),
        }
    } else {
        CoreError::Io {
            operation,
            kind: error.kind(),
        }
    }
}

fn sudo_capability_failure(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    stderr.contains("password is required")
        || stderr.contains("a terminal is required")
        || stderr.contains("no tty present")
        || stderr.contains("not allowed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_preview_never_contains_password_or_shell() {
        let spec = CommandSpec::new(KnownProgram::ChPasswd)
            .password_record(
                PasswordRecord::new(
                    super::super::UserName::new("alice").unwrap(),
                    SecretString::new("super-secret"),
                )
                .unwrap(),
            )
            .unwrap();
        let preview = spec.redacted_preview().render();
        assert!(preview.contains("chpasswd"));
        assert!(preview.contains("redacted"));
        assert!(!preview.contains("super-secret"));
        assert!(!preview.contains("bash"));
    }

    #[test]
    fn timeout_reaps_only_a_benign_test_helper() {
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args([
                "--ignored",
                "--exact",
                "sys::command::tests::benign_helper_waits",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let error = wait_with_timeout(&mut child, KnownProgram::Sudo, Duration::from_millis(20))
            .unwrap_err();
        assert!(matches!(error, CoreError::Timeout { .. }));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    #[ignore = "spawned only by timeout_reaps_only_a_benign_test_helper"]
    fn benign_helper_waits() {
        thread::sleep(Duration::from_secs(2));
    }
}
