//! Guest virtual-address-space metadata model.
//!
//! This module tracks guest virtual ranges and permissions only. It does not
//! reserve host memory at guest addresses, create hardware page tables, or
//! provide isolation from the host process.

use std::collections::BTreeMap;

use super::{loader::LoadPlan, stack::GuestStack};

pub const GUEST_PAGE_SIZE: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl GuestPermissions {
    pub const READ_ONLY: Self = Self {
        read: true,
        write: false,
        execute: false,
    };
    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
        execute: false,
    };

    fn merge(self, other: Self) -> Result<Self, &'static str> {
        let merged = Self {
            read: self.read || other.read,
            write: self.write || other.write,
            execute: self.execute || other.execute,
        };
        if merged.write && merged.execute {
            return Err("guest page would violate the W^X policy");
        }
        Ok(merged)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestRegionKind {
    ElfImage,
    Stack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestRegion {
    pub start: u64,
    pub end: u64,
    pub permissions: GuestPermissions,
    pub kind: GuestRegionKind,
}

impl GuestRegion {
    pub fn len(&self) -> u64 {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// A validated interval map of guest virtual addresses, not a real MMU map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuestAddressSpace {
    regions: Vec<GuestRegion>,
}

impl GuestAddressSpace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn regions(&self) -> &[GuestRegion] {
        &self.regions
    }

    /// Insert a page-aligned, non-empty range. Any overlap is rejected.
    pub fn reserve_region(
        &mut self,
        start: u64,
        end: u64,
        permissions: GuestPermissions,
        kind: GuestRegionKind,
    ) -> Result<(), &'static str> {
        if start >= end {
            return Err("guest region must have a non-empty range");
        }
        if start % GUEST_PAGE_SIZE != 0 || end % GUEST_PAGE_SIZE != 0 {
            return Err("guest region boundaries must be 4096-byte aligned");
        }
        if permissions.write && permissions.execute {
            return Err("writable-executable guest regions are rejected by W^X");
        }
        if self.regions.iter().any(|r| start < r.end && r.start < end) {
            return Err("guest virtual region overlaps an existing region");
        }
        self.regions.push(GuestRegion {
            start,
            end,
            permissions,
            kind,
        });
        self.regions.sort_by_key(|region| region.start);
        Ok(())
    }

    /// Build page-level metadata for the planned ELF image and initial stack.
    /// Adjacent pages with identical permissions are coalesced.
    pub fn from_load_plan(
        plan: &LoadPlan,
        stack: &GuestStack,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut pages: BTreeMap<u64, GuestPermissions> = BTreeMap::new();
        for segment in &plan.segments {
            if segment.virtual_start >= segment.virtual_end
                || segment.virtual_start % GUEST_PAGE_SIZE != 0
                || segment.virtual_end % GUEST_PAGE_SIZE != 0
            {
                return Err("ELF load plan contains invalid guest page boundaries".into());
            }
            let permissions = GuestPermissions {
                read: segment.readable,
                write: segment.writable,
                execute: segment.executable,
            };
            if permissions.write && permissions.execute {
                return Err("ELF load plan contains a writable-executable segment".into());
            }
            let mut page = segment.virtual_start;
            while page < segment.virtual_end {
                let merged = match pages.get(&page).copied() {
                    Some(existing) => existing.merge(permissions)?,
                    None => permissions,
                };
                pages.insert(page, merged);
                page = page
                    .checked_add(GUEST_PAGE_SIZE)
                    .ok_or("ELF guest page address overflow")?;
            }
        }
        if pages.is_empty() {
            return Err("ELF load plan contains no mapped pages".into());
        }

        let mut space = Self::new();
        let mut iter = pages.into_iter();
        let (mut start, mut permissions) = iter.next().ok_or("empty ELF page map")?;
        let mut end = start
            .checked_add(GUEST_PAGE_SIZE)
            .ok_or("ELF page range overflow")?;
        for (address, next_permissions) in iter {
            if address == end && next_permissions == permissions {
                end = address
                    .checked_add(GUEST_PAGE_SIZE)
                    .ok_or("ELF page range overflow")?;
            } else {
                space.reserve_region(start, end, permissions, GuestRegionKind::ElfImage)?;
                start = address;
                permissions = next_permissions;
                end = address
                    .checked_add(GUEST_PAGE_SIZE)
                    .ok_or("ELF page range overflow")?;
            }
        }
        space.reserve_region(start, end, permissions, GuestRegionKind::ElfImage)?;

        let stack_start = stack.stack_pointer() / GUEST_PAGE_SIZE * GUEST_PAGE_SIZE;
        let stack_end = stack
            .stack_end()
            .checked_add(GUEST_PAGE_SIZE - 1)
            .ok_or("guest stack page range overflow")?
            / GUEST_PAGE_SIZE
            * GUEST_PAGE_SIZE;
        space.reserve_region(
            stack_start,
            stack_end,
            GuestPermissions::READ_WRITE,
            GuestRegionKind::Stack,
        )?;
        if !space.contains(
            plan.entry_point,
            1,
            GuestPermissions {
                read: false,
                write: false,
                execute: true,
            },
        ) {
            return Err("ELF entry point is not covered by an executable guest region".into());
        }
        Ok(space)
    }

    /// Check that an entire non-empty range is covered by one region with the
    /// requested permissions. This checks metadata, not actual memory access.
    pub fn contains(&self, address: u64, length: u64, required: GuestPermissions) -> bool {
        if length == 0 {
            return false;
        }
        let Some(end) = address.checked_add(length) else {
            return false;
        };
        self.regions.iter().any(|region| {
            address >= region.start
                && end <= region.end
                && (!required.read || region.permissions.read)
                && (!required.write || region.permissions.write)
                && (!required.execute || region.permissions.execute)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_and_sorts_regions() {
        let mut space = GuestAddressSpace::new();
        space
            .reserve_region(
                0x4000,
                0x5000,
                GuestPermissions::READ_ONLY,
                GuestRegionKind::ElfImage,
            )
            .unwrap();
        space
            .reserve_region(
                0x1000,
                0x2000,
                GuestPermissions::READ_WRITE,
                GuestRegionKind::Stack,
            )
            .unwrap();
        assert_eq!(space.regions()[0].start, 0x1000);
    }

    #[test]
    fn rejects_empty_unaligned_and_overlapping_ranges() {
        let mut space = GuestAddressSpace::new();
        assert!(
            space
                .reserve_region(
                    0x1000,
                    0x1000,
                    GuestPermissions::READ_ONLY,
                    GuestRegionKind::ElfImage
                )
                .is_err()
        );
        assert!(
            space
                .reserve_region(
                    0x1001,
                    0x2000,
                    GuestPermissions::READ_ONLY,
                    GuestRegionKind::ElfImage
                )
                .is_err()
        );
        space
            .reserve_region(
                0x1000,
                0x3000,
                GuestPermissions::READ_ONLY,
                GuestRegionKind::ElfImage,
            )
            .unwrap();
        assert!(
            space
                .reserve_region(
                    0x2000,
                    0x4000,
                    GuestPermissions::READ_ONLY,
                    GuestRegionKind::Stack
                )
                .is_err()
        );
    }

    #[test]
    fn rejects_write_execute_regions() {
        let mut space = GuestAddressSpace::new();
        let wx = GuestPermissions {
            read: true,
            write: true,
            execute: true,
        };
        assert!(
            space
                .reserve_region(0x1000, 0x2000, wx, GuestRegionKind::ElfImage)
                .is_err()
        );
    }

    fn sample_stack() -> GuestStack {
        super::super::stack::build_initial_stack(&super::super::stack::GuestStackConfig {
            stack_top: 0x800000,
            argv: vec![b"/bin/demo".to_vec()],
            envp: vec![],
            execfn: b"/bin/demo".to_vec(),
            random: [7; 16],
            auxv: vec![],
        })
        .unwrap()
    }

    #[test]
    fn builds_elf_and_non_executable_stack_regions() {
        let plan = LoadPlan {
            architecture: "x86_64",
            load_bias: 0,
            entry_point: 0x400080,
            segments: vec![
                super::super::loader::SegmentMapping {
                    virtual_start: 0x400000,
                    virtual_end: 0x402000,
                    file_offset: 0,
                    file_size: 4096,
                    memory_size: 8192,
                    readable: true,
                    writable: false,
                    executable: true,
                },
                super::super::loader::SegmentMapping {
                    virtual_start: 0x402000,
                    virtual_end: 0x403000,
                    file_offset: 4096,
                    file_size: 0,
                    memory_size: 4096,
                    readable: true,
                    writable: true,
                    executable: false,
                },
            ],
        };
        let space = GuestAddressSpace::from_load_plan(&plan, &sample_stack()).unwrap();
        assert!(space.contains(
            0x400080,
            1,
            GuestPermissions {
                read: false,
                write: false,
                execute: true
            }
        ));
        assert!(space.contains(0x402000, 16, GuestPermissions::READ_WRITE));
        let stack = space
            .regions()
            .iter()
            .find(|region| region.kind == GuestRegionKind::Stack)
            .unwrap();
        assert!(stack.permissions.read && stack.permissions.write && !stack.permissions.execute);
    }

    #[test]
    fn rejects_overlapping_elf_pages_that_would_be_writable_executable() {
        let plan = LoadPlan {
            architecture: "x86_64",
            load_bias: 0,
            entry_point: 0x500000,
            segments: vec![
                super::super::loader::SegmentMapping {
                    virtual_start: 0x500000,
                    virtual_end: 0x501000,
                    file_offset: 0,
                    file_size: 1,
                    memory_size: 4096,
                    readable: true,
                    writable: false,
                    executable: true,
                },
                super::super::loader::SegmentMapping {
                    virtual_start: 0x500000,
                    virtual_end: 0x501000,
                    file_offset: 4096,
                    file_size: 1,
                    memory_size: 4096,
                    readable: true,
                    writable: true,
                    executable: false,
                },
            ],
        };
        assert!(GuestAddressSpace::from_load_plan(&plan, &sample_stack()).is_err());
    }

    #[test]
    fn checks_full_range_and_permissions() {
        let mut space = GuestAddressSpace::new();
        space
            .reserve_region(
                0x1000,
                0x3000,
                GuestPermissions::READ_WRITE,
                GuestRegionKind::Stack,
            )
            .unwrap();
        assert!(space.contains(0x1ff0, 16, GuestPermissions::READ_ONLY));
        assert!(!space.contains(0x2ff0, 32, GuestPermissions::READ_ONLY));
        assert!(!space.contains(
            0x1000,
            8,
            GuestPermissions {
                read: false,
                write: false,
                execute: true
            }
        ));
        assert!(!space.contains(u64::MAX - 2, 8, GuestPermissions::READ_ONLY));
        assert!(!space.contains(0x1000, 0, GuestPermissions::READ_ONLY));
    }
}
