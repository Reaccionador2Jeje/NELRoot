# NELRoot terminal interface

The CLI uses a compact terminal presentation layer in `src/ui.rs`.

## Visual behavior

- Cyan framed NELRoot banner and section headings.
- Green indicators for implemented host-side capabilities.
- Yellow indicators for unavailable or incomplete runtime capabilities.
- Red error marker for command failures.
- Automatic color only when stdout/stderr is an interactive terminal.
- Set `NO_COLOR=1` to disable ANSI styling in terminals, logs and scripts.
- Machine-readable output such as `nelroot engine status --json` remains plain JSON and is never decorated.

## Examples

```bash
nelroot
nelroot info
nelroot doctor
nelroot paths
nelroot elf inspect /path/to/program
nelroot elf plan /path/to/program
nelroot engine status
nelroot engine status --json
```

The ELF planner is informational only. It does not map memory or execute guest code. The visual status labels distinguish implemented inspection/planning features from the still-unimplemented guest runtime.
