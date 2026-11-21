//! Bounded, rootless guest-memory backing store.
//!
//! This is a byte-backed address-space model for loader and ABI development.
//! It does not reserve guest virtual addresses in the host process, create
//! hardware page tables, isolate memory, or execute guest instructions.

use super::address_space::{GuestAddressSpace, GuestPermissions, GuestRegion, GuestRegionKind};

/// Hard allocation ceiling for one in-process guest memory image.
pub const MAX_GUEST_MEMORY_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug)]
struct BackedRegion {
    metadata: GuestRegion,
    bytes: Vec<u8>,
}

/// Owns zero-initialized bytes for each validated guest region.
///
/// Accesses are checked against the guest virtual ranges and their declared
/// permissions. Each operation must fit entirely inside one region.
#[derive(Debug)]
pub struct GuestMemory {
    regions: Vec<BackedRegion>,
    total_bytes: usize,
}

impl GuestMemory {
    /// Allocate backing bytes for all regions in an address-space layout.
    /// The total allocation is capped to prevent unbounded host-memory use.
    pub fn from_address_space(
        address_space: &GuestAddressSpace,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if address_space.regions().is_empty() {
            return Err("cannot back an empty guest address space".into());
        }

        let mut total_bytes = 0_usize;
        let mut regions = Vec::with_capacity(address_space.regions().len());

        for region in address_space.regions() {
            if region.start >= region.end {
                return Err("guest memory region has an empty or inverted range".into());
            }
            let length = usize::try_from(region.len())?;
            total_bytes = total_bytes
                .checked_add(length)
                .ok_or("guest memory allocation size overflows")?;
            if total_bytes > MAX_GUEST_MEMORY_BYTES {
                return Err("guest memory exceeds the 512 MiB allocation limit".into());
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| "could not reserve host memory for guest region")?;
            bytes.resize(length, 0);
            regions.push(BackedRegion {
                metadata: region.clone(),
                bytes,
            });
        }

        Ok(Self {
            regions,
            total_bytes,
        })
    }

    /// Build byte-backed guest memory from a prepared ELF execution context.
    ///
    /// ELF pages are copied from the loader mapping (including execute-only
    /// pages and zero-filled BSS); the initial stack image is copied into its
    /// reserved guest stack range. This remains a data model, not execution.
    #[cfg(target_os = "linux")]
    pub fn from_execution_context(
        context: &super::loader::GuestExecutionContext,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut memory = Self::from_address_space(&context.address_space)?;

        for region in context
            .address_space
            .regions()
            .iter()
            .filter(|region| region.kind == GuestRegionKind::ElfImage)
        {
            let length = usize::try_from(region.len())?;
            let bytes = context.mapped.copy_virtual_bytes(region.start, length)?;
            memory.initialize(region.start, &bytes)?;
        }

        memory.initialize(context.stack.stack_pointer(), context.stack.bytes())?;
        Ok(memory)
    }

    /// Populate a reserved region during loader setup, before guest execution.
    fn initialize(&mut self, address: u64, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.is_empty() {
            return Err("guest initialization data must not be empty");
        }
        let region = self.region_for_mut(address, bytes.len())?;
        let offset = usize::try_from(address - region.metadata.start)
            .map_err(|_| "guest initialization offset does not fit host address space")?;
        region.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    /// Total host bytes allocated for guest regions.
    pub fn allocated_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Number of backed guest regions.
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Check that a non-empty range is covered by one region with permissions.
    pub fn validate_access(
        &self,
        address: u64,
        length: usize,
        required: GuestPermissions,
    ) -> Result<(), &'static str> {
        if length == 0 {
            return Err("guest memory access must not be empty");
        }
        let length_u64 = u64::try_from(length)
            .map_err(|_| "guest memory access length does not fit guest address space")?;
        let end = address
            .checked_add(length_u64)
            .ok_or("guest memory access range overflows")?;

        let region = self
            .regions
            .iter()
            .find(|region| address >= region.metadata.start && end <= region.metadata.end)
            .ok_or("guest memory access is outside a single backed region")?;

        let permissions = region.metadata.permissions;
        if required.read && !permissions.read {
            return Err("guest memory region is not readable");
        }
        if required.write && !permissions.write {
            return Err("guest memory region is not writable");
        }
        if required.execute && !permissions.execute {
            return Err("guest memory region is not executable");
        }
        Ok(())
    }

    /// Read guest bytes after checking the region's read permission.
    pub fn read(&self, address: u64, length: usize) -> Result<Vec<u8>, &'static str> {
        self.validate_access(
            address,
            length,
            GuestPermissions {
                read: true,
                write: false,
                execute: false,
            },
        )?;
        let region = self.region_for(address, length)?;
        let offset = usize::try_from(address - region.metadata.start)
            .map_err(|_| "guest memory offset does not fit host address space")?;
        Ok(region.bytes[offset..offset + length].to_vec())
    }

    /// Write guest bytes after checking the region's write permission.
    pub fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), &'static str> {
        self.validate_access(
            address,
            bytes.len(),
            GuestPermissions {
                read: false,
                write: true,
                execute: false,
            },
        )?;
        let region = self.region_for_mut(address, bytes.len())?;
        let offset = usize::try_from(address - region.metadata.start)
            .map_err(|_| "guest memory offset does not fit host address space")?;
        region.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    /// Fetch instruction bytes only from a region marked executable.
    /// Execute-only mappings can be fetched without granting guest read access.
    pub fn fetch_execute(&self, address: u64, max_length: usize) -> Result<Vec<u8>, &'static str> {
        if max_length == 0 {
            return Err("instruction fetch must not be empty");
        }
        let region = self
            .regions
            .iter()
            .find(|region| {
                address >= region.metadata.start
                    && address < region.metadata.end
                    && region.metadata.permissions.execute
            })
            .ok_or("instruction address is not executable")?;
        let offset = usize::try_from(address - region.metadata.start)
            .map_err(|_| "instruction offset does not fit host address space")?;
        let available = region.bytes.len().saturating_sub(offset);
        let length = available.min(max_length);
        if length == 0 {
            return Err("instruction fetch is outside backed bytes");
        }
        Ok(region.bytes[offset..offset + length].to_vec())
    }

    /// Populate bytes in an executable region for deterministic interpreter tests.
    #[cfg(test)]
    pub(crate) fn initialize_for_test(
        &mut self,
        address: u64,
        bytes: &[u8],
    ) -> Result<(), &'static str> {
        self.initialize(address, bytes)
    }

    /// Check whether a guest range is marked executable.
    ///
    /// This only validates metadata; it does not fetch or execute instructions.
    pub fn validate_execute(&self, address: u64, length: usize) -> Result<(), &'static str> {
        self.validate_access(
            address,
            length,
            GuestPermissions {
                read: false,
                write: false,
                execute: true,
            },
        )
    }

    /// Return the kind of the region containing the complete range.
    pub fn region_kind(
        &self,
        address: u64,
        length: usize,
    ) -> Result<GuestRegionKind, &'static str> {
        let region = self.region_for(address, length)?;
        Ok(region.metadata.kind)
    }

    fn region_for(&self, address: u64, length: usize) -> Result<&BackedRegion, &'static str> {
        if length == 0 {
            return Err("guest memory range must not be empty");
        }
        let length = u64::try_from(length).map_err(|_| "guest memory length overflow")?;
        let end = address
            .checked_add(length)
            .ok_or("guest memory range overflow")?;
        self.regions
            .iter()
            .find(|region| address >= region.metadata.start && end <= region.metadata.end)
            .ok_or("guest memory range is outside a single backed region")
    }

    fn region_for_mut(
        &mut self,
        address: u64,
        length: usize,
    ) -> Result<&mut BackedRegion, &'static str> {
        if length == 0 {
            return Err("guest memory range must not be empty");
        }
        let length = u64::try_from(length).map_err(|_| "guest memory length overflow")?;
        let end = address
            .checked_add(length)
            .ok_or("guest memory range overflow")?;
        self.regions
            .iter_mut()
            .find(|region| address >= region.metadata.start && end <= region.metadata.end)
            .ok_or("guest memory range is outside a single backed region")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_space() -> GuestAddressSpace {
        let mut space = GuestAddressSpace::new();
        space
            .reserve_region(
                0x4000,
                0x5000,
                GuestPermissions {
                    read: true,
                    write: false,
                    execute: true,
                },
                GuestRegionKind::ElfImage,
            )
            .unwrap();
        space
            .reserve_region(
                0x8000,
                0x9000,
                GuestPermissions::READ_WRITE,
                GuestRegionKind::Stack,
            )
            .unwrap();
        space
    }

    #[test]
    fn allocates_zeroed_backing_for_each_region() {
        let memory = GuestMemory::from_address_space(&sample_space()).unwrap();
        assert_eq!(memory.region_count(), 2);
        assert_eq!(memory.allocated_bytes(), 8192);
        assert_eq!(memory.read(0x8000, 4).unwrap(), vec![0; 4]);
    }

    #[test]
    fn reads_and_writes_only_permitted_regions() {
        let mut memory = GuestMemory::from_address_space(&sample_space()).unwrap();
        memory.write(0x8004, b"test").unwrap();
        assert_eq!(memory.read(0x8004, 4).unwrap(), b"test");
        assert!(memory.write(0x4000, b"x").is_err());
        assert_eq!(memory.read(0x4000, 1).unwrap(), vec![0]);
    }

    #[test]
    fn validates_execute_permission_without_executing() {
        let memory = GuestMemory::from_address_space(&sample_space()).unwrap();
        assert!(memory.validate_execute(0x4000, 4).is_ok());
        assert!(memory.validate_execute(0x8000, 4).is_err());
    }

    #[test]
    fn rejects_out_of_bounds_cross_region_and_empty_accesses() {
        let memory = GuestMemory::from_address_space(&sample_space()).unwrap();
        assert!(memory.read(0x8fff, 2).is_err());
        assert!(memory.read(0x5000, 1).is_err());
        assert!(memory.read(0x8000, 0).is_err());
        assert!(memory.read(u64::MAX, 2).is_err());
    }

    #[test]
    fn reports_region_kind() {
        let memory = GuestMemory::from_address_space(&sample_space()).unwrap();
        assert_eq!(
            memory.region_kind(0x4000, 1).unwrap(),
            GuestRegionKind::ElfImage
        );
        assert_eq!(
            memory.region_kind(0x8000, 1).unwrap(),
            GuestRegionKind::Stack
        );
    }

    #[test]
    fn rejects_empty_address_spaces() {
        assert!(GuestMemory::from_address_space(&GuestAddressSpace::new()).is_err());
    }
}
