# NELRoot Guest CPU Interpreter

NELRoot now includes a deliberately small x86_64 instruction interpreter in
`src/core/cpu.rs`. It operates exclusively on the bounded `GuestMemory`
model and never transfers control to host memory.

## Supported instruction subset

- `90`: NOP
- `31 C0`: XOR EAX, EAX
- `48 B8+r imm64`: MOV an immediate 64-bit value into one of RAX, RCX, RDX,
  RBX, RSP, RBP, RSI or RDI
- `48 83 C0/E8 ib`: ADD/SUB sign-extended immediate to RAX
- `50..57` / `58..5F`: PUSH/POP the eight legacy general-purpose registers
- `E8` / `E9`: relative CALL/JMP; `C3`: RET
- `EB`, `74`, `75`: short JMP, JZ and JNZ
- `0F 05`: SYSCALL, routed through `SyscallDispatcher`

The interpreter models the general-purpose register file, RIP and a basic
instruction counter. The run loop accepts a mandatory instruction budget and
returns when the guest process exits or the budget is exhausted. On Linux,
`X86_64GuestRuntime::from_execution_context` wires a prepared loader context,
byte-backed memory, initial RIP/RSP and syscall dispatcher into one runtime
facade with `step()` and `run(max_steps)` methods. The convenience constructor
`X86_64GuestRuntime::from_static_pie(path, stack_config)` runs the static-PIE
preparation pipeline (`prepare_static_pie` → execution context → guest runtime)
in one call. It accepts only the loader pipeline's static `ET_DYN` input and
still executes only the documented instruction subset.

## Memory and syscall boundary

Instruction fetch requires the guest region to have execute permission. The
fetch API can read execute-only backing bytes without granting guest read
permission. A syscall instruction maps the modeled x86_64 register convention
to the existing ABI dispatcher; it does not execute a host syscall.

## Important limitations

This is an initial interpreter milestone, not a complete x86_64 CPU or Linux
runtime. It does not yet support ELF startup execution, general arithmetic beyond the small ADD/SUB subset, most branches, full
stack/calling-convention semantics, complete flags semantics, interrupts,
signals, threads,
dynamic linking, or the full Linux syscall ABI. Unsupported opcodes stop with
an explicit error. The interpreter is currently suitable for tiny, controlled
smoke-test instruction streams only.

No guest instruction is executed natively by the host CPU, and no privileged
operation or host filesystem access is provided by this interpreter.
