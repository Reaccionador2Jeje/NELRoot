# NELRoot guest syscall ABI model

The first syscall boundary is implemented as a deterministic, host-independent
model in `src/core/abi.rs`. It accepts Linux x86_64 syscall register values and
returns Linux-style signed results (success values or negative errno values).

## Supported calls

| Syscall | Number | Behavior |
| --- | ---: | --- |
| `write` | 1 | Reads a bounded byte range from validated guest memory and appends it to an in-memory stdout (fd 1) or stderr (fd 2) capture buffer |
| `exit` | 60 | Marks the modeled process as exited with the low 8 bits of the requested status |
| `exit_group` | 231 | Uses the same single-process exit transition |

Unknown calls return `-ENOSYS`. Invalid descriptors return `-EBADF`,
invalid guest memory ranges return `-EFAULT`, invalid write lengths or calls
after exit return `-EINVAL`, and output-cap exhaustion returns `-ENOSPC`.

## Safety and current limits

- `write` never calls the host kernel and never writes to host files or terminal
  descriptors. Output remains in bounded Rust vectors owned by the model. A
  zero-byte write returns `0`; unsupported descriptors are rejected before
  guest-pointer access, matching the relevant Linux error precedence.
- Guest pointers are read only through `GuestMemory`, which checks the guest
  region and its read permission.
- A single write is limited to 1 MiB; captured output is limited to 4 MiB.
- There is no host file-descriptor table, filesystem access, stdin, networking,
  process creation, signal delivery, or arbitrary host syscall forwarding.
- The register layout and syscall numbers are explicitly Linux x86_64. Other
  architectures need their own ABI definitions and dispatch tables.
- The dispatcher can be initialized directly from a prepared
  `GuestExecutionContext` through `SyscallContext::from_execution_context`,
  which copies the prepared ELF and stack into `GuestMemory`. The
  `X86_64SyscallFrame` adapter maps RAX/RDI/RSI/RDX/R10/R8/R9 to the Linux
  syscall argument convention and writes the signed result back to RAX. The
  bounded x86_64 guest interpreter routes its `SYSCALL` instruction through
  this dispatcher. This is an in-process instruction model only: it is not a
  hardware trap, a complete Linux syscall implementation, or a general-purpose
  ELF execution environment.

## Local checks

Run `cargo fmt --all`, `cargo test --all-targets`, and
`cargo clippy --all-targets --all-features -- -D warnings` on a Linux
development host before treating this milestone as validated.
