# Rootless Guest Memory

NELRoot now has a bounded byte-backed guest-memory layer in
`src/core/guest_memory.rs`.

## Current API

- `GuestMemory::from_address_space` allocates zero-initialized backing bytes
  for a validated `GuestAddressSpace`.
- On Linux, `GuestMemory::from_execution_context` copies the prepared ELF
  image regions and initial stack bytes from a `GuestExecutionContext`.
- `read`, `write`, and `validate_execute` enforce the guest region's
  declared permissions and reject ranges that cross region boundaries.
- Loader initialization can populate read-only or executable regions before
  guest execution. This is deliberately separate from guest write access.
- A single guest-memory object is limited to 512 MiB. Allocation reservation
  failures are returned as errors.

## Loader handoff

The Linux preparation pipeline currently works in these stages:

1. Inspect and map a supported static ELF64 PIE in the host process.
2. Apply the supported x86_64 or AArch64 RELATIVE RELA relocations.
3. Build the ELF load plan and initial stack image.
4. Convert the prepared artifacts into a `GuestExecutionContext`.
5. Build a byte-backed `GuestMemory` from that context.

The guest memory image contains the ELF PT_LOAD page regions, including
zero-filled BSS, and the initial stack bytes at their guest virtual addresses.
The loader's internal copy path can populate execute-only pages without
weakening the permissions exposed to guest reads.

## Explicit limitations

This is not yet a Linux execution engine:

- Guest virtual addresses are represented by checked metadata and byte buffers;
  they are not installed at those addresses in the host process.
- The buffers do not provide hardware isolation, a guest MMU, or protection
  against bugs in the NELRoot host process.
- No guest CPU instructions are executed.
- No syscall dispatcher, dynamic linker, PT_INTERP support, TLS, signals, or
  complete process model is provided by this layer.
- The current ELF preparation path is host-native and static-PIE focused.

Do not describe this milestone as running arbitrary Linux binaries. It is the
memory and loader handoff foundation for a future execution backend.
