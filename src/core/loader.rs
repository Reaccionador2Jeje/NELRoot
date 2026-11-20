use super::elf::ElfInfo;

const PAGE_SIZE: u64 = 4096;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentMapping {
    pub virtual_start: u64,
    pub virtual_end: u64,
    pub file_offset: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadPlan {
    pub architecture: &'static str,
    pub load_bias: u64,
    pub entry_point: u64,
    pub segments: Vec<SegmentMapping>,
}

/// Build a conservative ELF mapping plan. This validates metadata only:
/// it does not allocate memory, map pages, load an interpreter, or execute code.
pub fn plan_load(info: &ElfInfo, load_bias: u64) -> Result<LoadPlan, Box<dyn std::error::Error>> {
    if info.file_type != ET_EXEC && info.file_type != ET_DYN {
        return Err("only ET_EXEC and ET_DYN ELF files can receive a load plan".into());
    }
    if info.machine != EM_X86_64 && info.machine != EM_AARCH64 {
        return Err("load planning currently supports x86_64 and AArch64 only".into());
    }
    if info.file_type == ET_EXEC && load_bias != 0 {
        return Err("ET_EXEC images require a zero load bias".into());
    }
    if load_bias % PAGE_SIZE != 0 {
        return Err("load bias must be aligned to 4096 bytes".into());
    }

    let mut segments = Vec::new();
    let mut executable_entry_contains = false;
    for header in info.program_headers.iter().filter(|header| header.kind == 1) {
        if header.file_size > header.memory_size {
            return Err("PT_LOAD file size exceeds memory size".into());
        }
        if header.alignment > 1
            && (header.virtual_address % header.alignment != header.offset % header.alignment)
        {
            return Err("PT_LOAD offset and virtual address are incongruent".into());
        }

        let start = header
            .virtual_address
            .checked_add(load_bias)
            .ok_or("PT_LOAD address plus load bias overflows")?;
        let end_unaligned = start
            .checked_add(header.memory_size)
            .ok_or("PT_LOAD memory range overflows")?;
        let virtual_start = start / PAGE_SIZE * PAGE_SIZE;
        let virtual_end = end_unaligned
            .checked_add(PAGE_SIZE - 1)
            .ok_or("PT_LOAD page range overflows")?
            / PAGE_SIZE
            * PAGE_SIZE;

        let readable = header.flags & 4 != 0;
        let writable = header.flags & 2 != 0;
        let executable = header.flags & 1 != 0;
        if writable && executable {
            return Err(
                "writable and executable PT_LOAD segments are rejected by the default W^X policy"
                    .into(),
            );
        }
        let executable_file_end = header
            .virtual_address
            .checked_add(header.file_size)
            .ok_or("PT_LOAD executable file range overflows")?;
        if executable
            && (header.virtual_address..executable_file_end).contains(&info.entry_point)
        {
            executable_entry_contains = true;
        }
        segments.push(SegmentMapping {
            virtual_start,
            virtual_end,
            file_offset: header.offset,
            file_size: header.file_size,
            memory_size: header.memory_size,
            readable,
            writable,
            executable,
        });
    }

    if segments.is_empty() {
        return Err("ELF image has no PT_LOAD segments".into());
    }
    if !executable_entry_contains {
        return Err("ELF entry point is not inside an executable PT_LOAD segment".into());
    }

    let entry_point = info
        .entry_point
        .checked_add(load_bias)
        .ok_or("ELF entry point plus load bias overflows")?;
    Ok(LoadPlan {
        architecture: info.architecture,
        load_bias,
        entry_point,
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::elf::inspect_elf_bytes;

    fn sample() -> ElfInfo {
        let mut bytes = vec![0_u8; 120];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
        bytes[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x400000_u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64_u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56_u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1_u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1_u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&5_u32.to_le_bytes()); // R + X
        bytes[72..80].copy_from_slice(&0_u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&0x400000_u64.to_le_bytes());
        bytes[96..104].copy_from_slice(&120_u64.to_le_bytes());
        bytes[104..112].copy_from_slice(&4096_u64.to_le_bytes());
        bytes[112..120].copy_from_slice(&4096_u64.to_le_bytes());
        inspect_elf_bytes(&bytes).unwrap()
    }

    #[test]
    fn creates_plan_without_executing_image() {
        let plan = plan_load(&sample(), 0).unwrap();
        assert_eq!(plan.entry_point, 0x400000);
        assert_eq!(plan.segments.len(), 1);
        assert!(plan.segments[0].readable);
        assert!(plan.segments[0].executable);
        assert!(!plan.segments[0].writable);
    }

    #[test]
    fn rejects_bias_for_fixed_executable() {
        assert!(plan_load(&sample(), PAGE_SIZE).is_err());
    }

    #[test]
    fn rejects_unaligned_bias() {
        let mut info = sample();
        info.file_type = ET_DYN;
        assert!(plan_load(&info, 1).is_err());
    }

    #[test]
    fn rejects_writable_executable_segment() {
        let mut info = sample();
        info.program_headers[0].flags = 7;
        assert!(plan_load(&info, 0).is_err());
    }

    #[test]
    fn rejects_entry_outside_executable_segment() {
        let mut info = sample();
        info.entry_point = 0x900000;
        assert!(plan_load(&info, 0).is_err());
    }

    #[test]
    fn applies_load_bias_to_dynamic_image() {
        let mut info = sample();
        info.file_type = ET_DYN;
        let plan = plan_load(&info, 0x2000).unwrap();
        assert_eq!(plan.entry_point, 0x402000);
        assert_eq!(plan.segments[0].virtual_start, 0x402000);
    }

    #[test]
    fn rejects_unsupported_machine() {
        let mut info = sample();
        info.machine = 3;
        assert!(plan_load(&info, 0).is_err());
    }

    #[test]
    fn rejects_load_bias_address_overflow() {
        let mut info = sample();
        info.file_type = ET_DYN;
        info.entry_point = u64::MAX - 0x100;
        assert!(plan_load(&info, 0x2000).is_err());
    }
}
