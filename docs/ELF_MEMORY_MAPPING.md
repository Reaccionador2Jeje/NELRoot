# ELF memory mapping milestone

NELRoot includes an experimental Linux-only memory mapper in `src/core/memory.rs`.

## Command

```bash
nelroot elf inspect ./program
nelroot elf plan ./program
nelroot elf map ./static-pie
nelroot elf relocate ./static-pie
```

`elf map` reserves anonymous memory in the current NELRoot process, copies each validated `PT_LOAD` file range, relies on zero-filled anonymous pages for the `p_memsz - p_filesz` BSS tail, and applies final per-page ELF permissions with `mprotect(2)`. The mapping is owned by an RAII object and is released with `munmap(2)` when the command finishes.

## Deliberate limits

- Linux hosts only; the ELF machine must match the current host.
- ELF64 little-endian static `ET_DYN` images only.
- Images with `PT_INTERP` are rejected; no dynamic linker is loaded.
- `elf relocate` supports only ELF64 `R_X86_64_RELATIVE` and `R_AARCH64_RELATIVE` RELA entries with symbol index zero. REL, PLT, symbol-based relocations, TLS, initial stack, `argv`, `envp`, and auxiliary vectors remain unsupported.
- The entry point is reported but never called.
- Input is bounded to 256 MiB and the reserved virtual-memory span to 512 MiB.
- Segment byte-range overlaps and writable-executable page combinations are rejected.
- PT_LOAD alignment must be a power of two (when greater than one), congruent with the file offset, and no larger than the host page size.
- `MappedElf::read_bytes` and `write_bytes` enforce PF_R/PF_W on every covered page and reject overflow or out-of-reservation ranges.

`elf relocate` applies supported relative relocations to the in-memory mapping after validating the complete relocation table and writable targets. This is still not a Linux process loader. It does not run guest instructions and does not replace the host kernel's ELF loader or syscall implementation used by the separate chroot runner.

## Relative relocation milestone

The Linux-only `nelroot elf relocate <file>` command maps a host-native static PIE and applies only architecture-native RELATIVE RELA entries:

- x86_64: `R_X86_64_RELATIVE` (type 8)
- AArch64: `R_AARCH64_RELATIVE` (type 1027)

The implementation requires a valid `PT_DYNAMIC` table terminated by `DT_NULL`, a complete `DT_RELA` / `DT_RELASZ` / `DT_RELAENT` tuple, 24-byte RELA entries, 8-byte-aligned writable targets, and a zero symbol index. It rejects unsupported REL/PLT relocation tables, unknown relocation types, out-of-range targets, and arithmetic overflow. It validates all entries before applying writes, so malformed relocation metadata does not cause a partially relocated image.

This command is a preparation and inspection milestone only: it does not execute the ELF entry point or establish a guest process.
