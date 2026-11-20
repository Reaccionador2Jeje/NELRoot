//! Small, architecture-explicit Linux syscall ABI model.
//!
//! This dispatcher is a testable boundary for future guest execution. It does
//! not enter the host kernel: write() targets an in-memory stdout/stderr sink,
//! and exit() only updates the modeled guest process state.

use super::guest_memory::GuestMemory;

/// Linux x86_64 syscall numbers currently understood by this model.
pub mod x86_64 {
    pub const WRITE: u64 = 1;
    pub const EXIT: u64 = 60;
    pub const EXIT_GROUP: u64 = 231;
}

/// Linux errno values returned as negative syscall results.
pub mod errno {
    pub const EBADF: i64 = 9;
    pub const EFAULT: i64 = 14;
    pub const EINVAL: i64 = 22;
    pub const ENOSYS: i64 = 38;
    pub const ENOSPC: i64 = 28;
}

/// Maximum bytes accepted by one virtual write call.
pub const MAX_SYSCALL_WRITE_BYTES: usize = 1024 * 1024;
/// Maximum captured output for one modeled guest process.
pub const MAX_CAPTURED_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

/// Register values presented at the syscall boundary (Linux x86_64 order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestSyscallRegisters {
    pub number: u64,
    pub args: [u64; 6],
}

impl GuestSyscallRegisters {
    pub fn new(number: u64, args: [u64; 6]) -> Self {
        Self { number, args }
    }
}

/// Modeled lifecycle state of one guest process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestProcessState {
    Running,
    Exited { code: i32 },
}

/// Captured output streams. These buffers are never connected to host files.
#[derive(Debug, Default)]
pub struct GuestOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl GuestOutput {
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    pub fn clear(&mut self) {
        self.stdout.clear();
        self.stderr.clear();
    }

    fn append(&mut self, fd: u64, bytes: &[u8]) -> Result<(), i64> {
        let target = match fd {
            1 => &mut self.stdout,
            2 => &mut self.stderr,
            _ => return Err(errno::EBADF),
        };
        let new_len = target.len().checked_add(bytes.len()).ok_or(errno::ENOSPC)?;
        if new_len > MAX_CAPTURED_OUTPUT_BYTES {
            return Err(errno::ENOSPC);
        }
        target.extend_from_slice(bytes);
        Ok(())
    }
}

/// State required to dispatch modeled guest syscalls.
#[derive(Debug)]
pub struct SyscallContext {
    pub memory: GuestMemory,
    pub process: GuestProcessState,
    pub output: GuestOutput,
}

impl SyscallContext {
    pub fn new(memory: GuestMemory) -> Self {
        Self {
            memory,
            process: GuestProcessState::Running,
            output: GuestOutput::default(),
        }
    }
}

/// Dispatches the currently supported Linux x86_64 syscall subset.
///
/// Results follow the Linux convention: non-negative success values or
/// negative errno values. Unknown calls return -ENOSYS.
#[derive(Debug, Default, Clone, Copy)]
pub struct SyscallDispatcher;

impl SyscallDispatcher {
    pub fn dispatch(
        &self,
        context: &mut SyscallContext,
        registers: GuestSyscallRegisters,
    ) -> i64 {
        if context.process != GuestProcessState::Running {
            return -errno::EINVAL;
        }

        match registers.number {
            x86_64::WRITE => self.write(context, registers.args[0], registers.args[1], registers.args[2]),
            x86_64::EXIT | x86_64::EXIT_GROUP => {
                context.process = GuestProcessState::Exited {
                    code: registers.args[0] as u8 as i32,
                };
                0
            }
            _ => -errno::ENOSYS,
        }
    }

    fn write(&self, context: &mut SyscallContext, fd: u64, address: u64, count: u64) -> i64 {
        let Ok(length) = usize::try_from(count) else {
            return -errno::EINVAL;
        };
        if length == 0 || length > MAX_SYSCALL_WRITE_BYTES {
            return -errno::EINVAL;
        }

        let bytes = match context.memory.read(address, length) {
            Ok(bytes) => bytes,
            Err(_) => return -errno::EFAULT,
        };
        match context.output.append(fd, &bytes) {
            Ok(()) => length as i64,
            Err(error) => -error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::address_space::{GuestAddressSpace, GuestPermissions, GuestRegionKind};

    fn context() -> SyscallContext {
        let mut layout = GuestAddressSpace::new();
        layout.reserve_region(
            0x1000,
            0x2000,
            GuestPermissions::READ_WRITE,
            GuestRegionKind::Stack,
        ).unwrap();
        let mut memory = GuestMemory::from_address_space(&layout).unwrap();
        memory.write(0x1100, b"hello NELRoot").unwrap();
        SyscallContext::new(memory)
    }

    #[test]
    fn write_captures_stdout_without_host_io() {
        let mut context = context();
        let result = SyscallDispatcher.dispatch(
            &mut context,
            GuestSyscallRegisters::new(x86_64::WRITE, [1, 0x1100, 13, 0, 0, 0]),
        );
        assert_eq!(result, 13);
        assert_eq!(context.output.stdout(), b"hello NELRoot");
        assert!(context.output.stderr().is_empty());
    }

    #[test]
    fn write_captures_stderr_separately() {
        let mut context = context();
        let result = SyscallDispatcher.dispatch(
            &mut context,
            GuestSyscallRegisters::new(x86_64::WRITE, [2, 0x1100, 5, 0, 0, 0]),
        );
        assert_eq!(result, 5);
        assert_eq!(context.output.stderr(), b"hello");
    }

    #[test]
    fn write_rejects_invalid_descriptors_and_memory() {
        let mut context = context();
        let bad_fd = SyscallDispatcher.dispatch(
            &mut context,
            GuestSyscallRegisters::new(x86_64::WRITE, [3, 0x1100, 5, 0, 0, 0]),
        );
        let bad_memory = SyscallDispatcher.dispatch(
            &mut context,
            GuestSyscallRegisters::new(x86_64::WRITE, [1, 0x9000, 5, 0, 0, 0]),
        );
        assert_eq!(bad_fd, -errno::EBADF);
        assert_eq!(bad_memory, -errno::EFAULT);
    }

    #[test]
    fn exit_transitions_process_and_blocks_later_calls() {
        let mut context = context();
        let exit = SyscallDispatcher.dispatch(
            &mut context,
            GuestSyscallRegisters::new(x86_64::EXIT_GROUP, [7, 0, 0, 0, 0, 0]),
        );
        let later = SyscallDispatcher.dispatch(
            &mut context,
            GuestSyscallRegisters::new(x86_64::WRITE, [1, 0x1100, 5, 0, 0, 0]),
        );
        assert_eq!(exit, 0);
        assert_eq!(context.process, GuestProcessState::Exited { code: 7 });
        assert_eq!(later, -errno::EINVAL);
        assert!(context.output.stdout().is_empty());
    }

    #[test]
    fn unknown_syscalls_return_enosys() {
        let mut context = context();
        let result = SyscallDispatcher.dispatch(
            &mut context,
            GuestSyscallRegisters::new(9999, [0; 6]),
        );
        assert_eq!(result, -errno::ENOSYS);
    }

    #[test]
    fn write_rejects_zero_and_oversized_requests() {
        let mut context = context();
        for count in [0, (MAX_SYSCALL_WRITE_BYTES + 1) as u64] {
            let result = SyscallDispatcher.dispatch(
                &mut context,
                GuestSyscallRegisters::new(x86_64::WRITE, [1, 0x1100, count, 0, 0, 0]),
            );
            assert_eq!(result, -errno::EINVAL);
        }
    }
}
