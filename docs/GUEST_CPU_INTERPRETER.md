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


## Experimental CLI entry point

On Linux x86_64, the bounded pipeline can be invoked with:

```sh
nelroot elf emulate ./static-program --max-steps 100000 -- arg1 arg2
```

The command accepts a static `ET_DYN` image without `PT_INTERP`, prepares its
load mappings and supported relative relocations, creates a guest stack, copies
the prepared image into bounded `GuestMemory`, and interprets instructions
until guest exit, an unsupported instruction, a memory fault, or the step
budget is reached. Guest `write` output is captured first and only forwarded
to the terminal after the interpreter stops.

This is an experimental smoke-test path, not a general-purpose Linux loader.
The generated stack currently has an empty environment and minimal auxiliary
vector; dynamic linking, normal libc startup, most x86_64 instructions, and
most Linux syscalls are unsupported. Do not expect ordinary compiled programs
to run. Use a purpose-built static test ELF containing only supported
instructions. The step budget is mandatory and bounded by the CLI option.
