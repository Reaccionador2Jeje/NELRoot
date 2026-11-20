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

        let mut next_rip = self.registers.rip;
        let mut outcome = StepOutcome::Continued;
        let length = match first {
            0x90 => 1, // NOP
            0x31 if bytes.get(1) == Some(&0xc0) => {
                self.registers.rax = 0;
                self.set_zero_flag(true);
                2 // XOR EAX, EAX
            }
            0x48 if bytes.get(1).is_some_and(|opcode| (0xb8..=0xbf).contains(opcode)) => {
                if bytes.len() < 10 { return Err(CpuError::TruncatedInstruction); }
                let value = u64::from_le_bytes(bytes[2..10].try_into().unwrap());
                self.set_register((bytes[1] - 0xb8) as usize, value);
                10
            }
            0x48 if bytes.get(1) == Some(&0x83) => {
                if bytes.len() < 4 { return Err(CpuError::TruncatedInstruction); }
                let operation = bytes[2];
                let immediate = bytes[3] as i8 as i64 as u64;
                match operation {
                    0xc0 => {
                        self.registers.rax = self.registers.rax.wrapping_add(immediate);
                        self.set_zero_flag(self.registers.rax == 0);
                    }
                    0xe8 => {
                        self.registers.rax = self.registers.rax.wrapping_sub(immediate);
                        self.set_zero_flag(self.registers.rax == 0);
                    }
                    _ => return Err(CpuError::UnsupportedInstruction { opcode: operation }),
                }
                4
            }
            0x50..=0x57 => {
                let value = self.get_register((first - 0x50) as usize);
                self.push_u64(context, value)?;
                1
            }
            0x58..=0x5f => {
                let value = self.pop_u64(context)?;
                self.set_register((first - 0x58) as usize, value);
                1
            }
            0xeb => {
                if bytes.len() < 2 { return Err(CpuError::TruncatedInstruction); }
                let displacement = bytes[1] as i8 as i64;
                next_rip = self.relative_target(self.registers.rip, 2, displacement)?;
                2
            }
            0xe9 => {
                if bytes.len() < 5 { return Err(CpuError::TruncatedInstruction); }
                let displacement = i32::from_le_bytes(bytes[1..5].try_into().unwrap()) as i64;
                next_rip = self.relative_target(self.registers.rip, 5, displacement)?;
                5
            }
            0x74 | 0x75 => {
                if bytes.len() < 2 { return Err(CpuError::TruncatedInstruction); }
                let zero = self.registers.rflags & (1 << 6) != 0;
                let take = if first == 0x74 { zero } else { !zero };
                if take {
                    next_rip = self.relative_target(
                        self.registers.rip, 2, bytes[1] as i8 as i64
                    )?;
                } else {
                    next_rip = self.registers.rip.checked_add(2).ok_or(CpuError::FetchFault)?;
                }
                2
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
                outcome = StepOutcome::Syscall { result };
                2
            }
            0x31 => return Err(CpuError::InvalidRegisterEncoding),
            opcode => return Err(CpuError::UnsupportedInstruction { opcode }),
        };

        self.registers.rip = if next_rip != self.registers.rip {
            next_rip
        } else {
            self.registers.rip.checked_add(length as u64).ok_or(CpuError::FetchFault)?
        };
        self.instructions_executed = self.instructions_executed.saturating_add(1);

        if let GuestProcessState::Exited { code } = context.process {
            Ok(StepOutcome::ProcessExited { code })
        } else {
            Ok(outcome)
        }
    }

    fn get_register(&self, index: usize) -> u64 {
        match index {
            0 => self.registers.rax, 1 => self.registers.rcx,
            2 => self.registers.rdx, 3 => self.registers.rbx,
            4 => self.registers.rsp, 5 => self.registers.rbp,
            6 => self.registers.rsi, 7 => self.registers.rdi,
            _ => unreachable!("register index is limited to 0..8"),
        }
    }

    fn set_zero_flag(&mut self, enabled: bool) {
        if enabled { self.registers.rflags |= 1 << 6; }
        else { self.registers.rflags &= !(1 << 6); }
    }

    fn relative_target(&self, ip: u64, length: u64, displacement: i64) -> Result<u64, CpuError> {
        let base = ip.checked_add(length).ok_or(CpuError::FetchFault)?;
        if displacement >= 0 {
            base.checked_add(displacement as u64).ok_or(CpuError::FetchFault)
        } else {
            base.checked_sub(displacement.unsigned_abs()).ok_or(CpuError::FetchFault)
        }
    }

    fn push_u64(&mut self, context: &mut SyscallContext, value: u64) -> Result<(), CpuError> {
        let new_rsp = self.registers.rsp.checked_sub(8).ok_or(CpuError::FetchFault)?;
        context.memory.write(new_rsp, &value.to_le_bytes()).map_err(|_| CpuError::FetchFault)?;
        self.registers.rsp = new_rsp;
        Ok(())
    }

    fn pop_u64(&mut self, context: &SyscallContext) -> Result<u64, CpuError> {
        let bytes = context.memory.read(self.registers.rsp, 8).map_err(|_| CpuError::FetchFault)?;
        let value = u64::from_le_bytes(bytes.try_into().map_err(|_| CpuError::FetchFault)?);
        self.registers.rsp = self.registers.rsp.checked_add(8).ok_or(CpuError::FetchFault)?;
        Ok(value)
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
    fn executes_arithmetic_and_conditional_branch() {
        // mov rax, 1; add rax, -1; jz +2; nop; nop
        let code = [0x48,0xb8,1,0,0,0,0,0,0,0, 0x48,0x83,0xc0,0xff, 0x74,0x01, 0x90, 0x90];
        let mut context = context_with_code(&code);
        let mut cpu = X86_64Interpreter::new(0x1000, 0x9000);
        assert_eq!(cpu.run(&mut context, &SyscallDispatcher, 8).unwrap(),
            CpuStopReason::StepLimitReached);
        assert_eq!(cpu.registers.rax, 0);
        assert_eq!(cpu.registers.rip, 0x1012);
    }

    #[test]
    fn push_and_pop_round_trip_register_value() {
        let code = [0x48,0xb8,0x2a,0,0,0,0,0,0,0, 0x50, 0x48,0xb8,0,0,0,0,0,0,0,0, 0x58];
        let mut context = context_with_code(&code);
        let mut cpu = X86_64Interpreter::new(0x1000, 0x9000);
        cpu.run(&mut context, &SyscallDispatcher, 4).unwrap();
        assert_eq!(cpu.registers.rax, 42);
        assert_eq!(cpu.registers.rsp, 0x9000);
    }

    #[test]
    fn relative_jump_changes_instruction_pointer() {
        let mut context = context_with_code(&[0xeb, 0x02, 0x90, 0x90, 0x90]);
        let mut cpu = X86_64Interpreter::new(0x1000, 0x9000);
        cpu.step(&mut context, &SyscallDispatcher).unwrap();
        assert_eq!(cpu.registers.rip, 0x1004);
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
