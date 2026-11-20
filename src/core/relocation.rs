//! Minimal, fail-closed ELF64 RELA relocation support for static PIE images.
//!
//! Only architecture-specific RELATIVE relocations are supported. Dynamic
//! linking, symbol lookup, PLT relocations, TLS and REL (non-RELA) are rejected.

use std::error::Error;

use super::{
    elf::{ElfInfo, ProgramHeader},
    memory::MappedElf,
};

const PT_DYNAMIC: u32 = 2;
const DT_NULL: i64 = 0;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;
const DT_REL: i64 = 17;
const DT_RELSZ: i64 = 18;
const DT_PLTRELSZ: i64 = 2;
const DT_JMPREL: i64 = 23;
const ELF64_DYNAMIC_ENTRY_SIZE: usize = 16;
const ELF64_RELA_ENTRY_SIZE: usize = 24;
const MAX_RELOCATIONS: usize = 1_000_000;
const MAX_DYNAMIC_TABLE_SIZE: usize = 16 * 1024 * 1024;

/// Apply supported relative relocations to an already mapped static PIE.
///
/// The function validates the complete relocation table and all destination
/// ranges before writing any relocated value. Unsupported relocation types
/// fail closed instead of being guessed or silently skipped.
pub fn apply_relative_relocations(
    mapped: &mut MappedElf,
    info: &ElfInfo,
) -> Result<usize, Box<dyn Error>> {
    if info.file_type != 3 || info.interpreter.is_some() {
        return Err("relative relocation stage accepts static ET_DYN images only".into());
    }
    if mapped.machine() != info.machine {
        return Err("ELF metadata machine does not match the mapped image".into());
    }

    let dynamic_segments: Vec<&ProgramHeader> = info
        .program_headers
        .iter()
        .filter(|header| header.kind == PT_DYNAMIC && header.memory_size > 0)
        .collect();

    if dynamic_segments.is_empty() {
        return Ok(0);
    }
    if dynamic_segments.len() != 1 {
        return Err("ELF image contains multiple PT_DYNAMIC segments".into());
    }

    let dynamic = dynamic_segments[0];
    if dynamic.file_size > dynamic.memory_size {
        return Err("PT_DYNAMIC file size exceeds its memory size".into());
    }
    let dynamic_length = usize::try_from(dynamic.file_size)?;
    if dynamic_length > MAX_DYNAMIC_TABLE_SIZE {
        return Err("PT_DYNAMIC exceeds the 16 MiB safety limit".into());
    }
    if dynamic_length % ELF64_DYNAMIC_ENTRY_SIZE != 0 {
        return Err("PT_DYNAMIC has an invalid entry table size".into());
    }

    let dynamic_offset =
        mapped.virtual_address_offset(dynamic.virtual_address, dynamic_length)?;
    let dynamic_bytes = mapped.read_bytes(dynamic_offset, dynamic_length)?;

    let mut rela_address = None;
    let mut rela_size = None;
    let mut rela_entry_size = None;
    let mut saw_null = false;

    for entry in dynamic_bytes.chunks_exact(ELF64_DYNAMIC_ENTRY_SIZE) {
        let tag = i64::from_le_bytes(entry[0..8].try_into()?);
        let value = u64::from_le_bytes(entry[8..16].try_into()?);
        if tag == DT_NULL {
            saw_null = true;
            break;
        }
        match tag {
            DT_RELA => set_once(&mut rela_address, value, "DT_RELA")?,
            DT_RELASZ => set_once(&mut rela_size, value, "DT_RELASZ")?,
            DT_RELAENT => set_once(&mut rela_entry_size, value, "DT_RELAENT")?,
            DT_REL | DT_RELSZ | DT_JMPREL | DT_PLTRELSZ => {
                if value != 0 {
                    return Err("ELF image requires unsupported REL or PLT relocations".into());
                }
            }
            _ => {}
        }
    }

    if !saw_null {
        return Err("PT_DYNAMIC is missing its DT_NULL terminator".into());
    }

    let any_rela = rela_address.is_some() || rela_size.is_some() || rela_entry_size.is_some();
    if !any_rela {
        return Ok(0);
    }

    let rela_address = rela_address.ok_or("DT_RELA is missing")?;
    let rela_size = rela_size.ok_or("DT_RELASZ is missing")?;
    let rela_entry_size = rela_entry_size.ok_or("DT_RELAENT is missing")?;
    if rela_entry_size != ELF64_RELA_ENTRY_SIZE as u64
        || rela_size % ELF64_RELA_ENTRY_SIZE as u64 != 0
    {
        return Err("ELF image has an unsupported RELA entry size".into());
    }

    let relocation_count = usize::try_from(rela_size / ELF64_RELA_ENTRY_SIZE as u64)?;
    if relocation_count > MAX_RELOCATIONS {
        return Err("ELF image exceeds the one-million relocation safety limit".into());
    }
    if relocation_count == 0 {
        return Ok(0);
    }

    let rela_offset = mapped.virtual_address_offset(rela_address, usize::try_from(rela_size)?)?;
    let rela_bytes = mapped.read_bytes(rela_offset, usize::try_from(rela_size)?)?;
    let mut pending = Vec::with_capacity(relocation_count);

    for entry in rela_bytes.chunks_exact(ELF64_RELA_ENTRY_SIZE) {
        let target_address = u64::from_le_bytes(entry[0..8].try_into()?);
        let relocation_info = u64::from_le_bytes(entry[8..16].try_into()?);
        let addend = i64::from_le_bytes(entry[16..24].try_into()?);

        if target_address % 8 != 0 {
            return Err("ELF relative relocation target is not 8-byte aligned".into());
        }

        let value = relative_relocation_value(
            info.machine,
            relocation_info,
            addend,
            mapped.load_bias(),
        )?;
        let target_offset = mapped.virtual_address_offset(target_address, 8)?;
        mapped.validate_write_range(target_offset, 8)?;
        pending.push((target_offset, value.to_le_bytes()));
    }

    // All entries and destinations have been validated. No partial relocation
    // occurs due to malformed metadata or an unsupported relocation type.
    for (offset, value) in &pending {
        mapped.write_bytes(*offset, value)?;
    }

    Ok(pending.len())
}

fn set_once(
    slot: &mut Option<u64>,
    value: u64,
    name: &'static str,
) -> Result<(), Box<dyn Error>> {
    if slot.replace(value).is_some() {
        return Err(format!("ELF dynamic table contains duplicate {name} entries").into());
    }
    Ok(())
}

fn relative_relocation_value(
    machine: u16,
    relocation_info: u64,
    addend: i64,
    load_bias: u64,
) -> Result<u64, Box<dyn Error>> {
    let symbol = relocation_info >> 32;
    let relocation_type = relocation_info as u32;
    let expected_type: u32 = match machine {
        62 => 8,     // R_X86_64_RELATIVE
        183 => 1027, // R_AARCH64_RELATIVE
        _ => return Err("relative relocations are unsupported for this ELF machine".into()),
    };

    if symbol != 0 || relocation_type != expected_type {
        return Err("ELF image contains an unsupported non-relative relocation".into());
    }

    let value = i128::from(load_bias) + i128::from(addend);
    if !(0..=i128::from(u64::MAX)).contains(&value) {
        return Err("ELF relative relocation result overflows 64 bits".into());
    }
    Ok(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_first_dynamic_tag_value() {
        let mut slot = None;
        set_once(&mut slot, 0x1234, "DT_RELA").unwrap();
        assert_eq!(slot, Some(0x1234));
    }

    #[test]
    fn rejects_duplicate_dynamic_tags() {
        let mut slot = None;
        set_once(&mut slot, 0x1234, "DT_RELA").unwrap();
        assert!(set_once(&mut slot, 0x5678, "DT_RELA").is_err());
    }

    #[test]
    fn rejects_unsupported_elf_machine() {
        assert!(relative_relocation_value(3, 8, 0, 0x1000).is_err());
    }

    #[test]
    fn computes_x86_64_relative_relocation() {
        assert_eq!(
            relative_relocation_value(62, 8, 0x1234, 0x1000).unwrap(),
            0x2234
        );
    }

    #[test]
    fn computes_aarch64_relative_relocation() {
        assert_eq!(
            relative_relocation_value(183, 1027, -0x100, 0x2000).unwrap(),
            0x1f00
        );
    }

    #[test]
    fn rejects_symbol_based_relocations() {
        assert!(relative_relocation_value(62, (1_u64 << 32) | 8, 0, 0x1000).is_err());
    }

    #[test]
    fn rejects_unknown_relocation_types() {
        assert!(relative_relocation_value(62, 1, 0, 0x1000).is_err());
    }

    #[test]
    fn rejects_negative_relocation_results() {
        assert!(relative_relocation_value(62, 8, -1, 0).is_err());
    }

    #[test]
    fn rejects_relocation_result_overflow() {
        assert!(relative_relocation_value(62, 8, 1, u64::MAX).is_err());
    }
}
