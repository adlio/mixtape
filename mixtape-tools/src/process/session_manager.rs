use mixtape_core::ToolError;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

lazy_static::lazy_static! {
    /// Patterns that indicate a process is waiting for input
    /// Matches common shell prompts, REPL prompts, and input indicators
    static ref PROMPT_PATTERNS: Regex = Regex::new(concat!(
        r"(?:",
        r">>>\s*$|",           // Python REPL
        r">\s*$|",             // Generic prompt, Node REPL
        r"\$\s*$|",            // Shell prompt
        r"#\s*$|",             // Root shell prompt
        r"%\s*$|",             // Zsh prompt
        r":\s*$|",             // Generic input prompt (e.g., "Enter name:")
        r"\?\s*$|",            // Question prompt
        r"password:\s*$|",     // Password prompt (case insensitive handled below)
        r"Password:\s*$|",
        r"\(yes/no\)\s*$|",    // Confirmation prompt
        r"\[Y/n\]\s*$|",       // Debian-style confirmation
        r"\[y/N\]\s*$|",
        r"irb.*>\s*$|",        // Ruby IRB
        r"pry.*>\s*$|",        // Ruby Pry
        r"iex.*>\s*$|",        // Elixir IEx
        r"scala>\s*$|",        // Scala REPL
        r"ghci>\s*$|",         // Haskell GHCi
        r"sqlite>\s*$|",       // SQLite
        r"mysql>\s*$|",        // MySQL
        r"postgres.*>\s*$|",   // PostgreSQL
        r"redis.*>\s*$",       // Redis CLI
        r")"
    )).expect("Invalid prompt regex");
}

/// State of a process session
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    /// Process is running and producing output
    Running,
    /// Process appears to be waiting for input (prompt detected)
    WaitingForInput,
    /// Process has exited
    Completed { exit_code: Option<i32> },
    /// Process exceeded its timeout
    TimedOut,
}

/// A managed process session
pub struct Session {
    pub pid: u32,
    pub command: String,
    pub process: Child,
    pub stdin: Option<ChildStdin>,
    pub output_buffer: Arc<Mutex<Vec<String>>>,
    pub state: ProcessState,
    pub created_at: Instant,
    pub timeout_ms: Option<u64>,
}

impl Session {
    pub fn new(
        pid: u32,
        command: String,
        process: Child,
        stdin: Option<ChildStdin>,
        timeout_ms: Option<u64>,
    ) -> Self {
        Self {
            pid,
            command,
            process,
            stdin,
            output_buffer: Arc::new(Mutex::new(Vec::new())),
            state: ProcessState::Running,
            created_at: Instant::now(),
            timeout_ms,
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.created_at.elapsed().as_millis() as u64
    }

    pub fn is_timed_out(&self) -> bool {
        if let Some(timeout) = self.timeout_ms {
            self.elapsed_ms() > timeout
        } else {
            false
        }
    }

    /// Check if the last line of output looks like a prompt waiting for input
    pub async fn is_waiting_for_input(&self) -> bool {
        let buffer = self.output_buffer.lock().await;
        if let Some(last_line) = buffer.last() {
            // Strip ANSI escape codes for cleaner matching
            let clean_line = strip_ansi_codes(last_line);
            PROMPT_PATTERNS.is_match(&clean_line)
        } else {
            false
        }
    }

    pub async fn check_status(&mut self) -> ProcessState {
        // Check if process has exited
        if let Ok(Some(status)) = self.process.try_wait() {
            self.state = ProcessState::Completed {
                exit_code: status.code(),
            };
            return self.state.clone();
        }

        // Check for timeout
        if self.is_timed_out() {
            self.state = ProcessState::TimedOut;
            return self.state.clone();
        }

        // Check if waiting for input (prompt detected)
        if self.is_waiting_for_input().await {
            self.state = ProcessState::WaitingForInput;
            return self.state.clone();
        }

        self.state = ProcessState::Running;
        self.state.clone()
    }
}

/// Strip ANSI escape codes from a string for cleaner pattern matching
pub(crate) fn strip_ansi_codes(s: &str) -> String {
    // Simple regex to strip ANSI escape sequences
    let ansi_regex = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").expect("Invalid ANSI regex");
    ansi_regex.replace_all(s, "").to_string()
}

/// Manager for process sessions
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<u32, Session>>>,
    next_pid: Arc<Mutex<u32>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_pid: Arc::new(Mutex::new(10000)),
        }
    }

    pub async fn create_session(
        &self,
        command: String,
        shell: Option<String>,
        timeout_ms: Option<u64>,
    ) -> Result<u32, ToolError> {
        let mut cmd = if let Some(shell_cmd) = shell {
            let mut c = Command::new(shell_cmd);
            c.arg("-c").arg(&command);
            c
        } else {
            #[cfg(unix)]
            {
                let mut c = Command::new("sh");
                c.arg("-c").arg(&command);
                c
            }
            #[cfg(windows)]
            {
                let mut c = Command::new("cmd");
                c.arg("/C").arg(&command);
                c
            }
        };

        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::from(format!("Failed to spawn process: {}", e)))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let mut next_pid = self.next_pid.lock().await;
        let pid = *next_pid;
        *next_pid += 1;

        let session = Session::new(pid, command, child, stdin, timeout_ms);
        let output_buffer = session.output_buffer.clone();

        // Spawn task to capture output
        if let Some(stdout) = stdout {
            let buffer = output_buffer.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    buffer.lock().await.push(line);
                }
            });
        }

        if let Some(stderr) = stderr {
            let buffer = output_buffer.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    buffer.lock().await.push(format!("[stderr] {}", line));
                }
            });
        }

        self.sessions.lock().await.insert(pid, session);
        Ok(pid)
    }

    pub async fn get_session(&self, pid: u32) -> Option<()> {
        self.sessions.lock().await.get(&pid).map(|_| ())
    }

    pub async fn read_output(&self, pid: u32, clear: bool) -> Result<Vec<String>, ToolError> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(&pid)
            .ok_or_else(|| format!("Session {} not found", pid))?;

        let mut buffer = session.output_buffer.lock().await;
        let output = buffer.clone();

        if clear {
            buffer.clear();
        }

        Ok(output)
    }

    pub async fn send_input(&self, pid: u32, input: &str) -> Result<(), ToolError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&pid)
            .ok_or_else(|| format!("Session {} not found", pid))?;

        if let Some(stdin) = &mut session.stdin {
            stdin
                .write_all(input.as_bytes())
                .await
                .map_err(|e| ToolError::from(format!("Failed to write to stdin: {}", e)))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| ToolError::from(format!("Failed to write newline: {}", e)))?;
            stdin
                .flush()
                .await
                .map_err(|e| ToolError::from(format!("Failed to flush stdin: {}", e)))?;
            Ok(())
        } else {
            Err("Process has no stdin".into())
        }
    }

    pub async fn check_status(&self, pid: u32) -> Result<ProcessState, ToolError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&pid)
            .ok_or_else(|| format!("Session {} not found", pid))?;

        Ok(session.check_status().await)
    }

    pub async fn terminate(&self, pid: u32, force: bool) -> Result<(), ToolError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&pid)
            .ok_or_else(|| format!("Session {} not found", pid))?;

        if force {
            session
                .process
                .kill()
                .await
                .map_err(|e| ToolError::from(format!("Failed to kill process: {}", e)))?;
        } else {
            #[cfg(unix)]
            {
                use nix::sys::signal::{self, Signal};
                use nix::unistd::Pid;
                if let Some(child_pid) = session.process.id() {
                    signal::kill(Pid::from_raw(child_pid as i32), Signal::SIGTERM)
                        .map_err(|e| ToolError::from(format!("Failed to send SIGTERM: {}", e)))?;
                } else {
                    return Err("Process has no PID".into());
                }
            }
            #[cfg(not(unix))]
            {
                session
                    .process
                    .kill()
                    .await
                    .map_err(|e| ToolError::from(format!("Failed to kill process: {}", e)))?;
            }
        }

        Ok(())
    }

    pub async fn list_sessions(&self) -> Vec<(u32, String, ProcessState, u64)> {
        let mut sessions = self.sessions.lock().await;
        let mut result = Vec::new();

        for session in sessions.values_mut() {
            let state = session.check_status().await;
            result.push((
                session.pid,
                session.command.clone(),
                state,
                session.elapsed_ms(),
            ));
        }

        result
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== strip_ansi_codes tests ====================

    #[test]
    fn test_strip_ansi_codes_empty_string() {
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn test_strip_ansi_codes_no_codes() {
        assert_eq!(strip_ansi_codes("Hello World"), "Hello World");
    }

    #[test]
    fn test_strip_ansi_codes_single_color() {
        // Green text
        assert_eq!(strip_ansi_codes("\x1b[32mGreen\x1b[0m"), "Green");
    }

    #[test]
    fn test_strip_ansi_codes_multiple_colors() {
        assert_eq!(
            strip_ansi_codes("\x1b[31mRed\x1b[0m and \x1b[34mBlue\x1b[0m"),
            "Red and Blue"
        );
    }

    #[test]
    fn test_strip_ansi_codes_complex_sequences() {
        // Bold, underline, colors with parameters
        assert_eq!(
            strip_ansi_codes("\x1b[1;4;31mBold underline red\x1b[0m"),
            "Bold underline red"
        );
    }

    #[test]
    fn test_strip_ansi_codes_256_colors() {
        // 256 color code (e.g., orange)
        assert_eq!(strip_ansi_codes("\x1b[38;5;208mOrange\x1b[0m"), "Orange");
    }

    #[test]
    fn test_strip_ansi_codes_cursor_movement() {
        // Cursor movement codes
        assert_eq!(strip_ansi_codes("\x1b[2Aup\x1b[3Bdown"), "updown");
    }

    #[test]
    fn test_strip_ansi_codes_preserves_other_escapes() {
        // Tab and newline should be preserved
        assert_eq!(
            strip_ansi_codes("Line1\nLine2\tTabbed"),
            "Line1\nLine2\tTabbed"
        );
    }

    // ==================== PROMPT_PATTERNS tests ====================

    #[test]
    fn test_prompt_patterns_python_repl() {
        assert!(PROMPT_PATTERNS.is_match(">>> "));
        assert!(PROMPT_PATTERNS.is_match(">>>"));
    }

    #[test]
    fn test_prompt_patterns_generic_prompt() {
        assert!(PROMPT_PATTERNS.is_match("> "));
        assert!(PROMPT_PATTERNS.is_match(">"));
    }

    #[test]
    fn test_prompt_patterns_shell_prompts() {
        assert!(PROMPT_PATTERNS.is_match("$ "));
        assert!(PROMPT_PATTERNS.is_match("# ")); // root
        assert!(PROMPT_PATTERNS.is_match("% ")); // zsh
    }

    #[test]
    fn test_prompt_patterns_input_prompts() {
        assert!(PROMPT_PATTERNS.is_match("Enter name: "));
        assert!(PROMPT_PATTERNS.is_match("? "));
        assert!(PROMPT_PATTERNS.is_match("password: "));
        assert!(PROMPT_PATTERNS.is_match("Password: "));
    }

    #[test]
    fn test_prompt_patterns_confirmation() {
        assert!(PROMPT_PATTERNS.is_match("Continue? (yes/no) "));
        assert!(PROMPT_PATTERNS.is_match("[Y/n] "));
        assert!(PROMPT_PATTERNS.is_match("[y/N] "));
    }

    #[test]
    fn test_prompt_patterns_language_repls() {
        assert!(PROMPT_PATTERNS.is_match("irb(main):001:0> "));
        assert!(PROMPT_PATTERNS.is_match("pry(main)> "));
        assert!(PROMPT_PATTERNS.is_match("iex(1)> "));
        assert!(PROMPT_PATTERNS.is_match("scala> "));
        assert!(PROMPT_PATTERNS.is_match("ghci> "));
    }

    #[test]
    fn test_prompt_patterns_database_prompts() {
        assert!(PROMPT_PATTERNS.is_match("sqlite> "));
        assert!(PROMPT_PATTERNS.is_match("mysql> "));
        assert!(PROMPT_PATTERNS.is_match("postgres=# "));
        assert!(PROMPT_PATTERNS.is_match("postgres=> "));
        assert!(PROMPT_PATTERNS.is_match("redis-cli> "));
    }

    #[test]
    fn test_prompt_patterns_non_prompts() {
        // Regular output should NOT match
        assert!(!PROMPT_PATTERNS.is_match("Hello World"));
        assert!(!PROMPT_PATTERNS.is_match("Processing..."));
        assert!(!PROMPT_PATTERNS.is_match("Error: something went wrong"));
        assert!(!PROMPT_PATTERNS.is_match("123456"));
        assert!(!PROMPT_PATTERNS.is_match("file.txt"));
    }

    #[test]
    fn test_prompt_patterns_partial_matches() {
        // Should match at end of line
        assert!(PROMPT_PATTERNS.is_match("user@host:~$ "));
        // Middle of line should not match (due to $ anchor)
        assert!(!PROMPT_PATTERNS.is_match("$ echo hello"));
    }

    // ==================== ProcessState tests ====================

    #[test]
    fn test_process_state_equality() {
        assert_eq!(ProcessState::Running, ProcessState::Running);
        assert_eq!(ProcessState::WaitingForInput, ProcessState::WaitingForInput);
        assert_eq!(ProcessState::TimedOut, ProcessState::TimedOut);
        assert_eq!(
            ProcessState::Completed { exit_code: Some(0) },
            ProcessState::Completed { exit_code: Some(0) }
        );
        assert_ne!(
            ProcessState::Completed { exit_code: Some(0) },
            ProcessState::Completed { exit_code: Some(1) }
        );
    }

    #[test]
    fn test_process_state_debug() {
        let running = ProcessState::Running;
        let completed = ProcessState::Completed { exit_code: Some(0) };
        assert!(format!("{:?}", running).contains("Running"));
        assert!(format!("{:?}", completed).contains("Completed"));
    }

    #[test]
    fn test_process_state_clone() {
        let state = ProcessState::Completed {
            exit_code: Some(42),
        };
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }

    // ==================== Session tests ====================

    #[tokio::test]
    async fn test_session_elapsed_ms() {
        let mut cmd = Command::new("echo");
        cmd.arg("test");
        let child = cmd.spawn().expect("Failed to spawn test process");

        let session = Session::new(1, "echo test".to_string(), child, None, None);

        // Should have some elapsed time
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(session.elapsed_ms() >= 10);
    }

    #[tokio::test]
    async fn test_session_timeout_none() {
        let mut cmd = Command::new("echo");
        cmd.arg("test");
        let child = cmd.spawn().expect("Failed to spawn test process");

        let session = Session::new(1, "echo test".to_string(), child, None, None);

        // No timeout set, should never be timed out
        assert!(!session.is_timed_out());
    }

    #[tokio::test]
    async fn test_session_timeout_not_exceeded() {
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        cmd.kill_on_drop(true);
        let child = cmd.spawn().expect("Failed to spawn test process");

        let session = Session::new(1, "sleep 10".to_string(), child, None, Some(60000));

        // Timeout is 60 seconds, should not be exceeded
        assert!(!session.is_timed_out());
    }

    #[tokio::test]
    async fn test_session_timeout_exceeded() {
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        cmd.kill_on_drop(true);
        let child = cmd.spawn().expect("Failed to spawn test process");

        // Very short timeout
        let session = Session::new(1, "sleep 10".to_string(), child, None, Some(1));

        // Wait for timeout
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(session.is_timed_out());
    }

    #[tokio::test]
    async fn test_session_is_waiting_for_input_empty_buffer() {
        let mut cmd = Command::new("echo");
        cmd.arg("test");
        let child = cmd.spawn().expect("Failed to spawn test process");

        let session = Session::new(1, "echo test".to_string(), child, None, None);

        // Empty buffer should return false
        assert!(!session.is_waiting_for_input().await);
    }

    #[tokio::test]
    async fn test_session_is_waiting_for_input_with_prompt() {
        let mut cmd = Command::new("echo");
        cmd.arg("test");
        let child = cmd.spawn().expect("Failed to spawn test process");

        let session = Session::new(1, "echo test".to_string(), child, None, None);

        // Add a prompt to the buffer
        session.output_buffer.lock().await.push(">>> ".to_string());

        assert!(session.is_waiting_for_input().await);
    }

    #[tokio::test]
    async fn test_session_is_waiting_for_input_no_prompt() {
        let mut cmd = Command::new("echo");
        cmd.arg("test");
        let child = cmd.spawn().expect("Failed to spawn test process");

        let session = Session::new(1, "echo test".to_string(), child, None, None);

        // Add regular output to the buffer
        session
            .output_buffer
            .lock()
            .await
            .push("Hello World".to_string());

        assert!(!session.is_waiting_for_input().await);
    }

    #[tokio::test]
    async fn test_session_is_waiting_for_input_ansi_prompt() {
        let mut cmd = Command::new("echo");
        cmd.arg("test");
        let child = cmd.spawn().expect("Failed to spawn test process");

        let session = Session::new(1, "echo test".to_string(), child, None, None);

        // Add a prompt with ANSI codes
        session
            .output_buffer
            .lock()
            .await
            .push("\x1b[32m>>> \x1b[0m".to_string());

        // Should still detect the prompt after stripping ANSI codes
        assert!(session.is_waiting_for_input().await);
    }

    #[tokio::test]
    async fn test_session_check_status_completed() {
        // Use a command that exits immediately
        let mut cmd = Command::new("echo");
        cmd.arg("done");
        let child = cmd.spawn().expect("Failed to spawn test process");

        let mut session = Session::new(1, "echo done".to_string(), child, None, None);

        // Poll for completion with timeout (avoid flaky fixed sleep)
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);
        loop {
            let state = session.check_status().await;
            if matches!(state, ProcessState::Completed { .. }) {
                return; // Success
            }
            if start.elapsed() > timeout {
                panic!("Process did not complete within timeout");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn test_session_check_status_timed_out() {
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        cmd.kill_on_drop(true);
        let child = cmd.spawn().expect("Failed to spawn test process");

        // Very short timeout
        let mut session = Session::new(1, "sleep 10".to_string(), child, None, Some(1));

        // Wait for timeout
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let state = session.check_status().await;
        assert_eq!(state, ProcessState::TimedOut);
    }

    // ==================== SessionManager tests ====================

    #[tokio::test]
    async fn test_session_manager_new() {
        let manager = SessionManager::new();
        let sessions = manager.list_sessions().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_session_manager_default() {
        let manager = SessionManager::default();
        let sessions = manager.list_sessions().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_session_manager_create_session() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo hello".to_string(), None, None)
            .await
            .expect("Failed to create session");

        assert!(pid >= 10000); // PIDs start at 10000
    }

    #[tokio::test]
    async fn test_session_manager_create_multiple_sessions() {
        let manager = SessionManager::new();

        let pid1 = manager
            .create_session("echo 1".to_string(), None, None)
            .await
            .expect("Failed to create session 1");

        let pid2 = manager
            .create_session("echo 2".to_string(), None, None)
            .await
            .expect("Failed to create session 2");

        // PIDs should be sequential
        assert_eq!(pid2, pid1 + 1);
    }

    #[tokio::test]
    async fn test_session_manager_create_session_with_custom_shell() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo hello".to_string(), Some("/bin/sh".to_string()), None)
            .await
            .expect("Failed to create session");

        assert!(pid >= 10000);
    }

    #[tokio::test]
    async fn test_session_manager_create_session_with_timeout() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("sleep 60".to_string(), None, Some(100))
            .await
            .expect("Failed to create session");

        // Wait for timeout
        tokio::time::sleep(tokio::time::Duration::from_millis(120)).await;

        let status = manager
            .check_status(pid)
            .await
            .expect("Failed to check status");
        assert_eq!(status, ProcessState::TimedOut);
    }

    #[tokio::test]
    async fn test_session_manager_get_session_exists() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo test".to_string(), None, None)
            .await
            .expect("Failed to create session");

        assert!(manager.get_session(pid).await.is_some());
    }

    #[tokio::test]
    async fn test_session_manager_get_session_not_exists() {
        let manager = SessionManager::new();
        assert!(manager.get_session(99999).await.is_none());
    }

    #[tokio::test]
    async fn test_session_manager_read_output() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo 'test output'".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Wait for output
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let output = manager
            .read_output(pid, false)
            .await
            .expect("Failed to read output");
        assert!(!output.is_empty());
        assert!(output.iter().any(|line| line.contains("test output")));
    }

    #[tokio::test]
    async fn test_session_manager_read_output_clear() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo 'test'".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Wait for output
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Read with clear
        let output1 = manager
            .read_output(pid, true)
            .await
            .expect("Failed to read output");
        assert!(!output1.is_empty());

        // Read again - should be empty since we cleared
        let output2 = manager
            .read_output(pid, false)
            .await
            .expect("Failed to read output");
        assert!(output2.is_empty());
    }

    #[tokio::test]
    async fn test_session_manager_read_output_not_found() {
        let manager = SessionManager::new();
        let result = manager.read_output(99999, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_session_manager_send_input() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("cat".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Send input
        manager
            .send_input(pid, "hello")
            .await
            .expect("Failed to send input");

        // Wait for echo
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let output = manager
            .read_output(pid, false)
            .await
            .expect("Failed to read output");
        assert!(output.iter().any(|line| line.contains("hello")));
    }

    #[tokio::test]
    async fn test_session_manager_send_input_not_found() {
        let manager = SessionManager::new();
        let result = manager.send_input(99999, "test").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_session_manager_check_status() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo hello".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Wait for completion
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let status = manager
            .check_status(pid)
            .await
            .expect("Failed to check status");
        assert!(matches!(status, ProcessState::Completed { .. }));
    }

    #[tokio::test]
    async fn test_session_manager_check_status_not_found() {
        let manager = SessionManager::new();
        let result = manager.check_status(99999).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_session_manager_terminate_force() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("sleep 60".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Force kill
        manager
            .terminate(pid, true)
            .await
            .expect("Failed to terminate");

        // Check it's completed
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let status = manager
            .check_status(pid)
            .await
            .expect("Failed to check status");
        assert!(matches!(status, ProcessState::Completed { .. }));
    }

    #[tokio::test]
    async fn test_session_manager_terminate_graceful() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("sleep 60".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Graceful terminate (SIGTERM)
        manager
            .terminate(pid, false)
            .await
            .expect("Failed to terminate");

        // Check it's completed
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let status = manager
            .check_status(pid)
            .await
            .expect("Failed to check status");
        assert!(matches!(status, ProcessState::Completed { .. }));
    }

    #[tokio::test]
    async fn test_session_manager_terminate_not_found() {
        let manager = SessionManager::new();
        let result = manager.terminate(99999, true).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_session_manager_list_sessions() {
        let manager = SessionManager::new();

        let pid1 = manager
            .create_session("sleep 10".to_string(), None, None)
            .await
            .expect("Failed to create session 1");

        let pid2 = manager
            .create_session("sleep 10".to_string(), None, None)
            .await
            .expect("Failed to create session 2");

        let sessions = manager.list_sessions().await;
        assert_eq!(sessions.len(), 2);

        let pids: Vec<u32> = sessions.iter().map(|(pid, _, _, _)| *pid).collect();
        assert!(pids.contains(&pid1));
        assert!(pids.contains(&pid2));

        // Cleanup
        let _ = manager.terminate(pid1, true).await;
        let _ = manager.terminate(pid2, true).await;
    }

    #[tokio::test]
    async fn test_session_manager_list_sessions_empty() {
        let manager = SessionManager::new();
        let sessions = manager.list_sessions().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_session_manager_stderr_capture() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo 'error message' >&2".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Wait for output
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let output = manager
            .read_output(pid, false)
            .await
            .expect("Failed to read output");
        // stderr lines should be prefixed with [stderr]
        assert!(output.iter().any(|line| line.contains("[stderr]")));
    }

    #[tokio::test]
    async fn test_session_manager_mixed_stdout_stderr() {
        let manager = SessionManager::new();
        let pid = manager
            .create_session("echo 'stdout' && echo 'stderr' >&2".to_string(), None, None)
            .await
            .expect("Failed to create session");

        // Wait for output
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let output = manager
            .read_output(pid, false)
            .await
            .expect("Failed to read output");
        assert!(output
            .iter()
            .any(|line| line.contains("stdout") && !line.contains("[stderr]")));
        assert!(output
            .iter()
            .any(|line| line.contains("[stderr]") && line.contains("stderr")));
    }
}
