//! Minimal, bounded x86_64 guest instruction interpreter.
//!
//! This is an interpreter for a deliberately tiny instruction subset, not a
//! native CPU backend. It fetches only from executable GuestMemory regions and
//! never jumps into host memory or invokes host syscalls directly.

use super::abi::{
    GuestProcessState, SyscallContext, SyscallDispatcher, X86_64SyscallFrame,
};

const MAX_INSTRUCTION_BYTES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct X86_64Registers {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuStopReason {
    ProcessExited { code: i32 },
    StepLimitReached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    Continued,
    Syscall { result: i64 },
    ProcessExited { code: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    FetchFault,
    UnsupportedInstruction { opcode: u8 },
    TruncatedInstruction,
    InvalidRegisterEncoding,
    ProcessNotRunning,
}

impl std::fmt::Display for CpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CpuError {}

/// A small interpreter for controlled x86_64 smoke-test programs.
#[derive(Debug, Default)]
pub struct X86_64Interpreter {
    pub registers: X86_64Registers,
    pub instructions_executed: u64,
}

impl X86_64Interpreter {
    pub fn new(entry_point: u64, stack_pointer: u64) -> Self {
        Self {
            registers: X86_64Registers {
                rip: entry_point,
                rsp: stack_pointer,
                ..X86_64Registers::default()
            },
            instructions_executed: 0,
        }
    }

    /// Execute one supported instruction. No host instruction pointer changes.
    pub fn step(
        &mut self,
        context: &mut SyscallContext,
        dispatcher: &SyscallDispatcher,
    ) -> Result<StepOutcome, CpuError> {
        if context.process != GuestProcessState::Running {
            return Err(CpuError::ProcessNotRunning);
        }
        let bytes = context.memory.fetch_execute(self.registers.rip, MAX_INSTRUCTION_BYTES)
            .map_err(|_| CpuError::FetchFault)?;
        let first = *bytes.first().ok_or(CpuError::TruncatedInstruction)?;

        let (length, outcome) = match first {
            0x90 => (1, StepOutcome::Continued), // NOP
            0x31 if bytes.get(1) == Some(&0xc0) => {
                self.registers.rax = 0; // XOR EAX, EAX (zero-extends into RAX)
                (2, StepOutcome::Continued)
            }
            0x48 if bytes.get(1).is_some_and(|opcode| (0xb8..=0xbf).contains(opcode)) => {
                if bytes.len() < 10 { return Err(CpuError::TruncatedInstruction); }
                let value = u64::from_le_bytes(bytes[2..10].try_into().unwrap());
                self.set_register((bytes[1] - 0xb8) as usize, value);
                (10, StepOutcome::Continued)
            }
            0x0f if bytes.get(1) == Some(&0x05) => {
                let mut frame = X86_64SyscallFrame {
                    rax: self.registers.rax,
                    rdi: self.registers.rdi,
                    rsi: self.registers.rsi,
                    rdx: self.registers.rdx,
                    r10: self.registers.r10,
                    r8: self.registers.r8,
                    r9: self.registers.r9,
                };
                let result = dispatcher.dispatch_x86_64_frame(context, &mut frame);
                self.registers.rax = frame.rax;
                (2, StepOutcome::Syscall { result })
            }
            0x31 => return Err(CpuError::InvalidRegisterEncoding),
            opcode => return Err(CpuError::UnsupportedInstruction { opcode }),
        };

        self.registers.rip = self.registers.rip.checked_add(length as u64)
            .ok_or(CpuError::FetchFault)?;
        self.instructions_executed = self.instructions_executed.saturating_add(1);

        if let GuestProcessState::Exited { code } = context.process {
            Ok(StepOutcome::ProcessExited { code })
        } else {
            Ok(outcome)
        }
    }

    fn set_register(&mut self, index: usize, value: u64) {
        match index {
            0 => self.registers.rax = value,
            1 => self.registers.rcx = value,
            2 => self.registers.rdx = value,
            3 => self.registers.rbx = value,
            4 => self.registers.rsp = value,
            5 => self.registers.rbp = value,
            6 => self.registers.rsi = value,
            7 => self.registers.rdi = value,
            _ => unreachable!("B8..BF encodes exactly eight registers"),
        }
    }

    /// Run at most max_steps instructions. The bound prevents runaway loops.
    pub fn run(
        &mut self,
        context: &mut SyscallContext,
        dispatcher: &SyscallDispatcher,
        max_steps: u64,
    ) -> Result<CpuStopReason, CpuError> {
        for _ in 0..max_steps {
            if let GuestProcessState::Exited { code } = context.process {
                return Ok(CpuStopReason::ProcessExited { code });
            }
            if matches!(self.step(context, dispatcher)?, StepOutcome::ProcessExited { .. }) {
                if let GuestProcessState::Exited { code } = context.process {
                    return Ok(CpuStopReason::ProcessExited { code });
                }
            }
        }
        Ok(CpuStopReason::StepLimitReached)
    }
}

/// Owns the initial CPU model and syscall state for one prepared x86_64
/// guest. Construction copies loader output into bounded GuestMemory; it does
/// not create a host process or enter native guest execution.
#[cfg(target_os = "linux")]
pub struct X86_64GuestRuntime {
    pub cpu: X86_64Interpreter,
    pub context: SyscallContext,
    dispatcher: SyscallDispatcher,
}

#[cfg(target_os = "linux")]
impl X86_64GuestRuntime {
    pub fn from_execution_context(
        prepared: &super::loader::GuestExecutionContext,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if prepared.architecture != "x86_64" {
            return Err("the initial guest interpreter supports x86_64 only".into());
        }
        let context = SyscallContext::from_execution_context(prepared)?;
        let cpu = X86_64Interpreter::new(prepared.entry_point, prepared.stack_pointer);
        Ok(Self { cpu, context, dispatcher: SyscallDispatcher })
    }

    pub fn step(&mut self) -> Result<StepOutcome, CpuError> {
        self.cpu.step(&mut self.context, &self.dispatcher)
    }

    pub fn run(&mut self, max_steps: u64) -> Result<CpuStopReason, CpuError> {
        self.cpu.run(&mut self.context, &self.dispatcher, max_steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::address_space::{GuestAddressSpace, GuestPermissions, GuestRegionKind};
    use crate::core::guest_memory::GuestMemory;

    fn context_with_code(code: &[u8]) -> SyscallContext {
        let mut layout = GuestAddressSpace::new();
        layout.reserve_region(0x1000, 0x2000,
            GuestPermissions { read: true, write: false, execute: true },
            GuestRegionKind::ElfImage).unwrap();
        layout.reserve_region(0x8000, 0x9000, GuestPermissions::READ_WRITE,
            GuestRegionKind::Stack).unwrap();
        let mut memory = GuestMemory::from_address_space(&layout).unwrap();
        memory.initialize_for_test(0x1000, code).unwrap();
        SyscallContext::new(memory)
    }

    #[test]
    fn interprets_nop_and_mov_rax_immediate() {
        let mut context = context_with_code(&[0x90, 0x48, 0xb8, 42, 0, 0, 0, 0, 0, 0]);
        let mut cpu = X86_64Interpreter::new(0x1000, 0x9000);
        let dispatcher = SyscallDispatcher;
        assert_eq!(cpu.step(&mut context, &dispatcher).unwrap(), StepOutcome::Continued);
        cpu.step(&mut context, &dispatcher).unwrap();
        assert_eq!(cpu.registers.rax, 42);
        assert_eq!(cpu.registers.rip, 0x100b);
    }

    #[test]
    fn executes_syscall_instruction_through_guest_abi() {
        // mov rax, 60; mov rdi, 7; syscall
        let code = [0x48,0xb8,60,0,0,0,0,0,0,0, 0x48,0xbf,7,0,0,0,0,0,0,0, 0x0f,0x05];
        let mut context = context_with_code(&code);
        let mut cpu = X86_64Interpreter::new(0x1000, 0x9000);
        let stop = cpu.run(&mut context, &SyscallDispatcher, 8).unwrap();
        assert_eq!(stop, CpuStopReason::ProcessExited { code: 7 });
        assert_eq!(context.process, GuestProcessState::Exited { code: 7 });
        assert_eq!(cpu.instructions_executed, 3);
    }

    #[test]
    fn refuses_to_fetch_from_non_executable_memory() {
        let mut context = context_with_code(&[0x90]);
        let mut cpu = X86_64Interpreter::new(0x8000, 0x9000);
        assert_eq!(cpu.step(&mut context, &SyscallDispatcher), Err(CpuError::FetchFault));
    }

    #[test]
    fn unsupported_instructions_stop_without_advancing() {
        let mut context = context_with_code(&[0xcc]);
        let mut cpu = X86_64Interpreter::new(0x1000, 0x9000);
        assert_eq!(cpu.step(&mut context, &SyscallDispatcher),
            Err(CpuError::UnsupportedInstruction { opcode: 0xcc }));
        assert_eq!(cpu.registers.rip, 0x1000);
    }

    #[test]
    fn run_enforces_instruction_budget() {
        let mut context = context_with_code(&[0x90; 32]);
        let mut cpu = X86_64Interpreter::new(0x1000, 0x9000);
        assert_eq!(cpu.run(&mut context, &SyscallDispatcher, 3).unwrap(),
            CpuStopReason::StepLimitReached);
        assert_eq!(cpu.instructions_executed, 3);
    }
}
