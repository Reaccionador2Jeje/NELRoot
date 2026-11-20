//! Bounded Linux ELF64 memory mapper.
//!
//! This maps static ET_DYN images into the current process for inspection and
//! future loader work. It deliberately does not transfer control to the image.

use std::{fs, io::Read, path::Path, ptr::NonNull};

use super::elf::{self, ElfInfo, ProgramHeader};

const PT_LOAD: u32 = 1;
const ET_DYN: u16 = 3;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const MAX_IMAGE_SIZE: usize = 256 * 1024 * 1024;
const MAX_MAPPING_SIZE: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PagePermissions(u8);

impl PagePermissions {
    fn merge(&mut self, flags: u32) -> Result<(), &'static str> {
        if flags & !(PF_R | PF_W | PF_X) != 0 {
            return Err("ELF PT_LOAD contains unsupported permission bits");
        }
        let mut bits = 0;
        if flags & PF_R != 0 {
            bits |= 1;
        }
        if flags & PF_W != 0 {
            bits |= 2;
        }
        if flags & PF_X != 0 {
            bits |= 4;
        }
        self.0 |= bits;
        if self.0 & 2 != 0 && self.0 & 4 != 0 {
            return Err("overlapping ELF segments would create a writable-executable page");
        }
        Ok(())
    }

    fn native(self) -> i32 {
        let mut protection = libc::PROT_NONE;
        if self.0 & 1 != 0 {
            protection |= libc::PROT_READ;
        }
        if self.0 & 2 != 0 {
            protection |= libc::PROT_WRITE;
        }
        if self.0 & 4 != 0 {
            protection |= libc::PROT_EXEC;
        }
        protection
    }

    fn staging(self) -> i32 {
        if self.0 == 0 {
            libc::PROT_NONE
        } else {
            libc::PROT_READ | libc::PROT_WRITE
        }
    }
}

/// Owns a mapped ELF image. Dropping it always unmaps the complete reservation.
pub struct MappedElf {
    base: NonNull<u8>,
    length: usize,
    min_virtual_address: u64,
    load_bias: u64,
    entry_offset: usize,
    page_size: usize,
    page_permissions: Vec<PagePermissions>,
}

impl std::fmt::Debug for MappedElf {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MappedElf")
            .field("base", &self.base)
            .field("length", &self.length)
            .field("load_bias", &format_args!("{:#x}", self.load_bias))
            .field("entry_address", &format_args!("{:#x}", self.entry_address()))
            .finish()
    }
}

impl MappedElf {
    /// Map a bounded static PIE (ET_DYN without PT_INTERP) into this process.
    /// The image is never executed. Dynamic linking and relocations are not performed.
    pub fn map_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let metadata = fs::metadata(path)?;
        if metadata.len() > MAX_IMAGE_SIZE as u64 {
            return Err("ELF image exceeds the 256 MiB mapping input limit".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take((MAX_IMAGE_SIZE + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_IMAGE_SIZE {
            return Err("ELF image grew beyond the 256 MiB mapping input limit".into());
        }
        let info = elf::inspect_elf_bytes(&bytes)?;
        Self::map_bytes(&bytes, &info)
    }

    fn map_bytes(bytes: &[u8], info: &ElfInfo) -> Result<Self, Box<dyn std::error::Error>> {
        if info.file_type != ET_DYN {
            return Err("the first memory mapper supports static ET_DYN images only".into());
        }
        if info.interpreter.is_some() {
            return Err(
                "dynamic ELF images with PT_INTERP need the dynamic linker stage and cannot be mapped yet"
                    .into(),
            );
        }
        if !host_machine_matches(info.machine) {
            return Err("ELF machine does not match the current host architecture".into());
        }

        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size <= 0 {
            return Err("could not determine the host memory page size".into());
        }
        let page_size = usize::try_from(page_size)?;
        if !page_size.is_power_of_two() {
            return Err("host memory page size is not a power of two".into());
        }
        let page_size_u64 = page_size as u64;

        let load_segments: Vec<&ProgramHeader> = info
            .program_headers
            .iter()
            .filter(|header| header.kind == PT_LOAD && header.memory_size > 0)
            .collect();
        if load_segments.is_empty() {
            return Err("ELF image contains no non-empty PT_LOAD segments".into());
        }

        let mut ranges = Vec::with_capacity(load_segments.len());
        let mut min_page = u64::MAX;
        let mut max_page = 0_u64;
        for header in &load_segments {
            if header.alignment > page_size_u64 {
                return Err(
                    "PT_LOAD alignment exceeds the host page size supported by this mapper".into(),
                );
            }
            if header.file_size > header.memory_size {
                return Err("PT_LOAD file size exceeds memory size".into());
            }
            let file_end = header
                .offset
                .checked_add(header.file_size)
                .ok_or("PT_LOAD file range overflows")?;
            if file_end > bytes.len() as u64 {
                return Err("PT_LOAD file range extends beyond the input".into());
            }
            let memory_end = header
                .virtual_address
                .checked_add(header.memory_size)
                .ok_or("PT_LOAD virtual range overflows")?;
            let start_page = align_down(header.virtual_address, page_size_u64);
            let end_page = align_up(memory_end, page_size_u64)?;
            min_page = min_page.min(start_page);
            max_page = max_page.max(end_page);
            ranges.push((header.virtual_address, memory_end, start_page, end_page));
        }

        let span_u64 = max_page
            .checked_sub(min_page)
            .ok_or("invalid ELF mapping span")?;
        let length = usize::try_from(span_u64)?;
        if length == 0 || length > MAX_MAPPING_SIZE {
            return Err(
                "ELF virtual memory span is empty or exceeds the 512 MiB mapping limit".into(),
            );
        }
        let page_count = length / page_size;
        let mut page_permissions = vec![PagePermissions::default(); page_count];

        for (header, (_, _, start_page, end_page)) in load_segments.iter().zip(&ranges) {
            let first = usize::try_from((start_page - min_page) / page_size_u64)?;
            let last = usize::try_from((end_page - min_page) / page_size_u64)?;
            for page in &mut page_permissions[first..last] {
                page.merge(header.flags)?;
            }
        }

        // PT_LOAD memory ranges may share a page, but their actual byte ranges
        // must not overlap: copying one segment over another would be ambiguous.
        for (left_index, left) in ranges.iter().enumerate() {
            for right in ranges.iter().skip(left_index + 1) {
                if left.0 < right.1 && right.0 < left.1 {
                    return Err("ELF PT_LOAD memory ranges overlap".into());
                }
            }
        }

        let mut entry_is_executable = false;
        for (header, (start, end, _, _)) in load_segments.iter().zip(&ranges) {
            if header.flags & PF_X != 0
                && info.entry_point >= *start
                && info.entry_point < *end
            {
                entry_is_executable = true;
            }
        }
        if !entry_is_executable {
            return Err("ELF entry point is not inside an executable PT_LOAD segment".into());
        }

        // SAFETY: mmap reserves a fresh private anonymous range. It is not
        // exposed to guest code and is owned by MappedElf after validation.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().into());
        }
        if raw.is_null() {
            // SAFETY: the zero-address mapping still represents our reservation.
            unsafe {
                libc::munmap(raw, length);
            }
            return Err("mmap returned a null address".into());
        }
        let base =
            NonNull::new(raw.cast::<u8>()).expect("non-null mmap address checked above");

        let entry_offset = usize::try_from(
            info.entry_point
                .checked_sub(min_page)
                .ok_or("ELF entry point precedes the mapped image")?,
        )?;

        let mut mapped = Self {
            base,
            length,
            min_virtual_address: min_page,
            load_bias: 0,
            entry_offset,
            page_size,
            page_permissions,
        };

        // Temporarily make only PT_LOAD pages writable for segment population.
        mapped.protect_runs(true)?;

        for header in &load_segments {
            let destination_offset = usize::try_from(header.virtual_address - min_page)?;
            let source_start = usize::try_from(header.offset)?;
            let source_end = usize::try_from(header.offset + header.file_size)?;
            let copy_len = source_end - source_start;
            if copy_len > 0 {
                // SAFETY: both ranges were checked against the mapped span and
                // input bytes above; staging permissions make destination writable.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr().add(source_start),
                        mapped.base.as_ptr().add(destination_offset),
                        copy_len,
                    );
                }
            }
            // Anonymous mmap pages are zero-filled, so p_memsz - p_filesz is BSS.
        }

        mapped.protect_runs(false)?;

        let base_address = mapped.base.as_ptr() as usize;
        let load_bias = (base_address as u64)
            .checked_sub(min_page)
            .ok_or("mapped base is lower than the ELF virtual base")?;
        mapped.load_bias = load_bias;
        Ok(mapped)
    }

    fn protect_runs(&mut self, staging: bool) -> Result<(), Box<dyn std::error::Error>> {
        let mut index = 0;
        while index < self.page_permissions.len() {
            let permissions = self.page_permissions[index];
            let mut end = index + 1;
            while end < self.page_permissions.len()
                && self.page_permissions[end] == permissions
            {
                end += 1;
            }
            let address = unsafe { self.base.as_ptr().add(index * self.page_size) };
            let length = (end - index) * self.page_size;
            let protection = if staging {
                permissions.staging()
            } else {
                permissions.native()
            };
            // SAFETY: address and length describe pages within our mmap reservation.
            if unsafe { libc::mprotect(address.cast(), length, protection) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            index = end;
        }
        Ok(())
    }

    pub fn base_address(&self) -> usize {
        self.base.as_ptr() as usize
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn load_bias(&self) -> u64 {
        self.load_bias
    }

    pub fn entry_address(&self) -> usize {
        self.base_address() + self.entry_offset
    }

    pub fn read_byte(&self, offset: usize) -> Result<u8, &'static str> {
        if offset >= self.length {
            return Err("mapped ELF offset is outside the reservation");
        }
        let page = offset / self.page_size;
        if self.page_permissions[page].0 & 1 == 0 {
            return Err("mapped ELF page is not readable");
        }
        // SAFETY: offset is in bounds and its page has PF_R permission.
        Ok(unsafe { self.base.as_ptr().add(offset).read() })
    }

    pub fn min_virtual_address(&self) -> u64 {
        self.min_virtual_address
    }
}

impl Drop for MappedElf {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns this mmap reservation.
        unsafe {
            libc::munmap(self.base.as_ptr().cast(), self.length);
        }
    }
}

fn align_down(value: u64, alignment: u64) -> u64 {
    value / alignment * alignment
}

fn align_up(value: u64, alignment: u64) -> Result<u64, &'static str> {
    value
        .checked_add(alignment - 1)
        .map(|value| align_down(value, alignment))
        .ok_or("ELF page alignment overflows")
}

fn host_machine_matches(machine: u16) -> bool {
    match std::env::consts::ARCH {
        "x86_64" => machine == 62,
        "aarch64" => machine == 183,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn static_pie() -> (Vec<u8>, ElfInfo) {
        let mut bytes = vec![0_u8; 120];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&ET_DYN.to_le_bytes());
        let machine = if std::env::consts::ARCH == "aarch64" {
            183_u16
        } else {
            62_u16
        };
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x100_u64.to_le_bytes());
        bytes[32..40].copy_from_slice(&64_u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56_u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1_u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[68..72].copy_from_slice(&(PF_R | PF_X).to_le_bytes());
        bytes[72..80].copy_from_slice(&0_u64.to_le_bytes());
        bytes[80..88].copy_from_slice(&0_u64.to_le_bytes());
        let file_size = bytes.len() as u64;
        bytes[96..104].copy_from_slice(&file_size.to_le_bytes());
        bytes[104..112].copy_from_slice(&8192_u64.to_le_bytes());
        bytes[112..120].copy_from_slice(&4096_u64.to_le_bytes());
        let info = elf::inspect_elf_bytes(&bytes).unwrap();
        (bytes, info)
    }

    #[test]
    fn maps_static_pie_and_zero_fills_bss() {
        let (bytes, info) = static_pie();
        let mapped = MappedElf::map_bytes(&bytes, &info).unwrap();
        assert!(mapped.length() >= 8192);
        assert!(mapped.entry_address() >= mapped.base_address());
        assert_eq!(mapped.read_byte(0).unwrap(), 0x7f);
        let bss_offset = bytes.len();
        for offset in bss_offset..bss_offset + 64 {
            assert_eq!(mapped.read_byte(offset).unwrap(), 0);
        }
    }

    #[test]
    fn rejects_non_pie_executable() {
        let (bytes, mut info) = static_pie();
        info.file_type = 2;
        assert!(MappedElf::map_bytes(&bytes, &info).is_err());
    }

    #[test]
    fn rejects_entry_outside_executable_segment() {
        let (bytes, mut info) = static_pie();
        info.entry_point = 0x3000;
        assert!(MappedElf::map_bytes(&bytes, &info).is_err());
    }

    #[test]
    fn rejects_non_host_machine() {
        let (bytes, mut info) = static_pie();
        info.machine = if std::env::consts::ARCH == "aarch64" { 62 } else { 183 };
        assert!(MappedElf::map_bytes(&bytes, &info).is_err());
    }

    #[test]
    fn rejects_writable_executable_segment() {
        let (bytes, mut info) = static_pie();
        info.program_headers[0].flags = PF_R | PF_W | PF_X;
        assert!(MappedElf::map_bytes(&bytes, &info).is_err());
    }
}
