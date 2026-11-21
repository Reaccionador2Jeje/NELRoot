use std::{fs, path::Path};

const ELF_HEADER_SIZE: usize = 64;
const ELF64_PROGRAM_HEADER_SIZE: usize = 56;
const MAX_INSPECT_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ProgramHeader {
    pub kind: u32,
    pub flags: u32,
    pub offset: u64,
    pub virtual_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub alignment: u64,
}

#[derive(Debug, Clone)]
pub struct ElfInfo {
    pub file_type: u16,
    pub machine: u16,
    pub architecture: &'static str,
    pub entry_point: u64,
    pub interpreter: Option<String>,
    pub program_headers: Vec<ProgramHeader>,
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, Box<dyn std::error::Error>> {
    let value: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or("ELF field is outside the file")?
        .try_into()?;
    Ok(u16::from_le_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn std::error::Error>> {
    let value: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or("ELF field is outside the file")?
        .try_into()?;
    Ok(u32::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, Box<dyn std::error::Error>> {
    let value: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or("ELF field is outside the file")?
        .try_into()?;
    Ok(u64::from_le_bytes(value))
}

fn architecture_name(machine: u16) -> &'static str {
    match machine {
        3 => "x86",
        40 => "ARM",
        62 => "x86_64",
        183 => "AArch64",
        243 => "RISC-V",
        _ => "unknown",
    }
}

/// Parse and validate an ELF64 little-endian executable without loading or running it.
pub fn inspect_elf_bytes(bytes: &[u8]) -> Result<ElfInfo, Box<dyn std::error::Error>> {
    if bytes.len() < ELF_HEADER_SIZE {
        return Err("file is too small to contain an ELF64 header".into());
    }
    if &bytes[0..4] != b"\x7fELF" {
        return Err("invalid ELF magic".into());
    }
    if bytes[4] != 2 {
        return Err("only ELF64 files are supported by the inspection parser".into());
    }
    if bytes[5] != 1 {
        return Err("only little-endian ELF files are supported".into());
    }
    if bytes[6] != 1 {
        return Err("unsupported ELF identification version".into());
    }

    let file_type = read_u16(bytes, 16)?;
    let machine = read_u16(bytes, 18)?;
    if read_u32(bytes, 20)? != 1 {
        return Err("unsupported ELF header version".into());
    }

    let entry_point = read_u64(bytes, 24)?;
    let program_header_offset = read_u64(bytes, 32)?;
    let header_size = read_u16(bytes, 52)? as usize;
    let program_header_entry_size = read_u16(bytes, 54)? as usize;
    let program_header_count = read_u16(bytes, 56)? as usize;

    if header_size < ELF_HEADER_SIZE || header_size > bytes.len() {
        return Err("invalid ELF header size".into());
    }
    if program_header_count > 0 && program_header_entry_size < ELF64_PROGRAM_HEADER_SIZE {
        return Err("ELF program-header entry is smaller than the ELF64 minimum".into());
    }

    let table_start = usize::try_from(program_header_offset)?;
    let table_size = program_header_entry_size
        .checked_mul(program_header_count)
        .ok_or("ELF program-header table size overflow")?;
    let table_end = table_start
        .checked_add(table_size)
        .ok_or("ELF program-header table offset overflow")?;
    if table_end > bytes.len() {
        return Err("ELF program-header table extends beyond the file".into());
    }

    let mut program_headers = Vec::with_capacity(program_header_count);
    let mut interpreter = None;
    for index in 0..program_header_count {
        let offset = table_start + index * program_header_entry_size;
        let header = ProgramHeader {
            kind: read_u32(bytes, offset)?,
            flags: read_u32(bytes, offset + 4)?,
            offset: read_u64(bytes, offset + 8)?,
            virtual_address: read_u64(bytes, offset + 16)?,
            file_size: read_u64(bytes, offset + 32)?,
            memory_size: read_u64(bytes, offset + 40)?,
            alignment: read_u64(bytes, offset + 48)?,
        };

        if header.kind == 1 {
            let segment_end = header
                .offset
                .checked_add(header.file_size)
                .ok_or("ELF load segment file range overflow")?;
            if segment_end > bytes.len() as u64 {
                return Err("ELF load segment extends beyond the file".into());
            }
            if header.memory_size < header.file_size {
                return Err("ELF load segment memory size is smaller than its file size".into());
            }
            header
                .virtual_address
                .checked_add(header.memory_size)
                .ok_or("ELF load segment virtual address overflow")?;
            if header.alignment > 1 {
                if !header.alignment.is_power_of_two() {
                    return Err("ELF load segment alignment is not a power of two".into());
                }
                if header.virtual_address % header.alignment != header.offset % header.alignment {
                    return Err(
                        "ELF load segment file offset and virtual address are not congruent".into(),
                    );
                }
            }
        } else if header.kind == 3 {
            if interpreter.is_some() {
                return Err("ELF file contains multiple PT_INTERP segments".into());
            }
            if header.file_size == 0 || header.file_size > 4096 {
                return Err("ELF interpreter path has an invalid size".into());
            }
            let start = usize::try_from(header.offset)?;
            let end = usize::try_from(
                header
                    .offset
                    .checked_add(header.file_size)
                    .ok_or("ELF interpreter range overflow")?,
            )?;
            let raw = bytes
                .get(start..end)
                .ok_or("ELF interpreter extends beyond the file")?;
            let nul = raw
                .iter()
                .position(|byte| *byte == 0)
                .ok_or("ELF interpreter path is not NUL-terminated")?;
            if nul == 0 || raw[nul + 1..].iter().any(|byte| *byte != 0) {
                return Err("ELF interpreter path has invalid trailing data".into());
            }
            let path = std::str::from_utf8(&raw[..nul])?;
            if !path.starts_with('/') {
                return Err("ELF interpreter path must be absolute".into());
            }
            interpreter = Some(path.to_owned());
        }
        program_headers.push(header);
    }

    Ok(ElfInfo {
        file_type,
        machine,
        architecture: architecture_name(machine),
        entry_point,
        interpreter,
        program_headers,
    })
}

/// Read a bounded file and inspect its ELF metadata. This never executes the file.
pub fn inspect_file(path: &Path) -> Result<ElfInfo, Box<dyn std::error::Error>> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_INSPECT_SIZE {
        return Err("ELF inspection is limited to files up to 256 MiB".into());
    }
    let bytes = fs::read(path)?;
    inspect_elf_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_elf64() -> Vec<u8> {
        let mut bytes = vec![0_u8; ELF_HEADER_SIZE + ELF64_PROGRAM_HEADER_SIZE];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2_u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&62_u16.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x400000_u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&(ELF_HEADER_SIZE as u64).to_le_bytes());
        bytes[52..54].copy_from_slice(&(ELF_HEADER_SIZE as u16).to_le_bytes());
        bytes[54..56].copy_from_slice(&(ELF64_PROGRAM_HEADER_SIZE as u16).to_le_bytes());
        bytes[56..58].copy_from_slice(&1_u16.to_le_bytes());

        let ph = ELF_HEADER_SIZE;
        bytes[ph..ph + 4].copy_from_slice(&1_u32.to_le_bytes());
        bytes[ph + 4..ph + 8].copy_from_slice(&5_u32.to_le_bytes());
        bytes[ph + 8..ph + 16].copy_from_slice(&0_u64.to_le_bytes());
        bytes[ph + 16..ph + 24].copy_from_slice(&0x400000_u64.to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&0_u64.to_le_bytes());
        bytes[ph + 40..ph + 48].copy_from_slice(&4096_u64.to_le_bytes());
        bytes[ph + 48..ph + 56].copy_from_slice(&4096_u64.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_valid_elf64_metadata() {
        let info = inspect_elf_bytes(&minimal_elf64()).unwrap();
        assert_eq!(info.architecture, "x86_64");
        assert_eq!(info.entry_point, 0x400000);
        assert_eq!(info.program_headers.len(), 1);
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut bytes = minimal_elf64();
        bytes[0] = 0;
        assert!(inspect_elf_bytes(&bytes).is_err());
    }

    #[test]
    fn rejects_out_of_bounds_program_header_table() {
        let mut bytes = minimal_elf64();
        bytes[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(inspect_elf_bytes(&bytes).is_err());
    }

    fn elf_with_interpreter(path: &[u8]) -> Vec<u8> {
        let mut bytes = minimal_elf64();
        let second_header = ELF_HEADER_SIZE + ELF64_PROGRAM_HEADER_SIZE;
        let interpreter_offset = second_header + ELF64_PROGRAM_HEADER_SIZE;
        bytes.resize(interpreter_offset + path.len() + 1, 0);
        bytes[56..58].copy_from_slice(&2_u16.to_le_bytes());

        bytes[second_header..second_header + 4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[second_header + 8..second_header + 16]
            .copy_from_slice(&(interpreter_offset as u64).to_le_bytes());
        bytes[second_header + 32..second_header + 40]
            .copy_from_slice(&((path.len() + 1) as u64).to_le_bytes());
        bytes[second_header + 40..second_header + 48]
            .copy_from_slice(&((path.len() + 1) as u64).to_le_bytes());
        bytes[interpreter_offset..interpreter_offset + path.len()].copy_from_slice(path);
        bytes
    }

    #[test]
    fn reads_absolute_pt_interp_path() {
        let bytes = elf_with_interpreter(b"/lib/ld-linux-aarch64.so.1");
        let info = inspect_elf_bytes(&bytes).unwrap();
        assert_eq!(
            info.interpreter.as_deref(),
            Some("/lib/ld-linux-aarch64.so.1")
        );
    }

    #[test]
    fn rejects_relative_pt_interp_path() {
        let bytes = elf_with_interpreter(b"ld-linux.so");
        assert!(inspect_elf_bytes(&bytes).is_err());
    }

    #[test]
    fn rejects_misaligned_load_segment() {
        let mut bytes = minimal_elf64();
        let ph = ELF_HEADER_SIZE;
        bytes[ph + 8..ph + 16].copy_from_slice(&1_u64.to_le_bytes());
        assert!(inspect_elf_bytes(&bytes).is_err());
    }

    #[test]
    fn rejects_load_segment_beyond_file() {
        let mut bytes = minimal_elf64();
        let ph = ELF_HEADER_SIZE;
        bytes[ph + 32..ph + 40].copy_from_slice(&1024_u64.to_le_bytes());
        bytes[ph + 40..ph + 48].copy_from_slice(&2048_u64.to_le_bytes());
        assert!(inspect_elf_bytes(&bytes).is_err());
    }
}
