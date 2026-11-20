# ELF memory mapping milestone

NELRoot includes an experimental Linux-only memory mapper in `src/core/memory.rs`.

## Command

```bash
nelroot elf inspect ./program
nelroot elf plan ./program
nelroot elf map ./static-pie
```

`elf map` reserves anonymous memory in the current NELRoot process, copies each validated `PT_LOAD` file range, relies on zero-filled anonymous pages for the `p_memsz - p_filesz` BSS tail, and applies final per-page ELF permissions with `mprotect(2)`. The mapping is owned by an RAII object and is released with `munmap(2)` when the command finishes.

## Deliberate limits

- Linux hosts only; the ELF machine must match the current host.
- ELF64 little-endian static `ET_DYN` images only.
- Images with `PT_INTERP` are rejected; no dynamic linker is loaded.
- No relocations, TLS, initial stack, `argv`, `envp`, or auxiliary vector are built.
- The entry point is reported but never called.
- Input is bounded to 256 MiB and the reserved virtual-memory span to 512 MiB.
- Segment byte-range overlaps and writable-executable page combinations are rejected.
- Segment alignment larger than the host page size is rejected by this first implementation.

This is a real memory mapping primitive, not yet a Linux process loader. It does not run guest instructions and does not replace the host kernel's ELF loader or syscall implementation used by the separate chroot runner.
