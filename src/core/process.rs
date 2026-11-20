//! Owned child-process supervision primitives.
//!
//! This module deliberately does not create namespaces or claim guest
//! isolation. It provides the process lifecycle building block used by the
//! future namespace-backed runtime.

use std::{
    io,
    process::{Child, Command, ExitStatus},
};

/// Observable lifecycle state of a supervised child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Exited,
}

/// Owns a child process and guarantees that dropping the supervisor does not
/// leave that child running in the background.
///
/// Callers should prefer `wait`, `try_wait`, or `terminate` to make the
/// lifecycle explicit. Drop performs best-effort kill-and-reap as a final
/// cleanup safeguard.
#[derive(Debug)]
pub struct SupervisedProcess {
    child: Child,
    state: ProcessState,
}

impl SupervisedProcess {
    /// Spawn a command and take ownership of its child process.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command.spawn()?;
        Ok(Self {
            child,
            state: ProcessState::Running,
        })
    }

    /// Host PID of the owned child process.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Current lifecycle state, updated by wait/poll operations.
    pub fn state(&self) -> ProcessState {
        self.state
    }

    /// Non-blocking child poll. Returns the exit status once the child exits.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.state = ProcessState::Exited;
        }
        Ok(status)
    }

    /// Wait until the child exits and return its status.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.state = ProcessState::Exited;
        Ok(status)
    }

    /// Stop the child if it is still running, then reap it.
    ///
    /// This is an immediate kill, not graceful signal delivery. A later
    /// runtime milestone will add guest signal forwarding and shutdown policy.
    pub fn terminate(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.try_wait()? {
            return Ok(status);
        }

        if let Err(error) = self.child.kill() {
            // Resolve the exit-vs-kill race before returning an error.
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            return Err(error);
        }

        self.wait()
    }
}

impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        if self.state == ProcessState::Running {
            // Best-effort cleanup: never panic from Drop.
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.state = ProcessState::Exited;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    #[test]
    fn wait_propagates_child_exit_status() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 23"]);
        let mut process = SupervisedProcess::spawn(&mut command).unwrap();

        let status = process.wait().unwrap();
        assert_eq!(status.code(), Some(23));
        assert_eq!(process.state(), ProcessState::Exited);
    }

    #[test]
    fn try_wait_reports_running_then_exited() {
        let mut command = Command::new("sleep");
        command.arg("0.2");
        let mut process = SupervisedProcess::spawn(&mut command).unwrap();

        assert_eq!(process.state(), ProcessState::Running);
        assert!(process.try_wait().unwrap().is_none());
        let status = process.wait().unwrap();
        assert!(status.success());
        assert_eq!(process.state(), ProcessState::Exited);
    }

    #[test]
    fn terminate_kills_and_reaps_child() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let mut process = SupervisedProcess::spawn(&mut command).unwrap();
        thread::sleep(Duration::from_millis(20));

        let status = process.terminate().unwrap();
        assert!(!status.success());
        assert_eq!(process.state(), ProcessState::Exited);
    }
}
