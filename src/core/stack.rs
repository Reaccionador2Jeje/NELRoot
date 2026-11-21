//! Bounded ELF64 initial guest-stack image builder.
//!
//! This creates bytes and guest virtual pointers only. It does not map a host
//! stack, enter guest mode, or transfer control to an ELF entry point.

use std::error::Error;

const WORD_SIZE: usize = 8;
const STACK_ALIGNMENT: u64 = 16;
const MAX_STACK_SIZE: usize = 8 * 1024 * 1024;
const MAX_ARGUMENTS: usize = 4096;
const MAX_ENVIRONMENT: usize = 4096;

pub const AT_NULL: u64 = 0;
pub const AT_RANDOM: u64 = 25;
pub const AT_EXECFN: u64 = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuxEntry {
    pub kind: u64,
    pub value: u64,
}

#[derive(Debug, Clone)]
pub struct GuestStackConfig {
    /// 16-byte-aligned exclusive upper address of the guest stack.
    pub stack_top: u64,
    /// Argument and environment strings without terminating NUL bytes.
    pub argv: Vec<Vec<u8>>,
    pub envp: Vec<Vec<u8>>,
    /// Executable pathname used for AT_EXECFN; may differ from argv[0].
    pub execfn: Vec<u8>,
    /// Entropy supplied by the host adapter; copied into the guest stack.
    pub random: [u8; 16],
    /// Entries such as AT_PHDR, AT_PHENT, AT_PHNUM and AT_PAGESZ.
    /// AT_NULL, AT_RANDOM and AT_EXECFN are reserved and generated here.
    pub auxv: Vec<AuxEntry>,
}

/// Fully materialized stack image. bytes[0] resides at stack_pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestStack {
    stack_pointer: u64,
    bytes: Vec<u8>,
    argv_addresses: Vec<u64>,
    envp_addresses: Vec<u64>,
    random_address: u64,
    execfn_address: u64,
}

impl GuestStack {
    pub fn stack_pointer(&self) -> u64 {
        self.stack_pointer
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn stack_end(&self) -> u64 {
        self.stack_pointer + self.bytes.len() as u64
    }

    pub fn argv_addresses(&self) -> &[u64] {
        &self.argv_addresses
    }

    pub fn envp_addresses(&self) -> &[u64] {
        &self.envp_addresses
    }

    pub fn random_address(&self) -> u64 {
        self.random_address
    }

    pub fn execfn_address(&self) -> u64 {
        self.execfn_address
    }
}

/// Build argc, argv + NULL, envp + NULL, auxv + AT_NULL, then strings and AT_RANDOM.
pub fn build_initial_stack(config: &GuestStackConfig) -> Result<GuestStack, Box<dyn Error>> {
    if config.stack_top == 0 || config.stack_top % STACK_ALIGNMENT != 0 {
        return Err("guest stack top must be non-zero and 16-byte aligned".into());
    }
    if config.argv.is_empty() {
        return Err("ELF initial stack requires at least argv[0]".into());
    }
    if config.argv.len() > MAX_ARGUMENTS || config.envp.len() > MAX_ENVIRONMENT {
        return Err(
            "guest stack argument or environment count exceeds the 4096-entry limit".into(),
        );
    }
    if config
        .auxv
        .iter()
        .any(|entry| entry.kind == AT_NULL || entry.kind == AT_RANDOM || entry.kind == AT_EXECFN)
    {
        return Err("AT_NULL, AT_RANDOM and AT_EXECFN are reserved by the stack builder".into());
    }
    if config.execfn.is_empty() || config.execfn.contains(&0) {
        return Err("AT_EXECFN pathname must be non-empty and contain no NUL bytes".into());
    }

    let mut payload_size = 16_usize
        .checked_add(config.execfn.len())
        .and_then(|size| size.checked_add(1))
        .ok_or("guest execfn size overflow")?;

    for value in config.argv.iter().chain(config.envp.iter()) {
        if value.contains(&0) {
            return Err("guest argv/envp strings cannot contain embedded NUL bytes".into());
        }
        payload_size = payload_size
            .checked_add(value.len())
            .and_then(|size| size.checked_add(1))
            .ok_or("guest stack string size overflow")?;
    }

    let aux_count = config
        .auxv
        .len()
        .checked_add(3)
        .ok_or("auxv count overflow")?;
    let word_count = 1_usize
        .checked_add(config.argv.len())
        .and_then(|count| count.checked_add(1))
        .and_then(|count| count.checked_add(config.envp.len()))
        .and_then(|count| count.checked_add(1))
        .and_then(|count| count.checked_add(aux_count.checked_mul(2)?))
        .ok_or("guest stack pointer table size overflow")?;
    let table_size = word_count
        .checked_mul(WORD_SIZE)
        .ok_or("guest stack table size overflow")?;
    let unaligned_size = table_size
        .checked_add(payload_size)
        .ok_or("guest stack size overflow")?;
    let total_size = unaligned_size
        .checked_add(15)
        .ok_or("guest stack alignment overflow")?
        & !15;

    if total_size > MAX_STACK_SIZE {
        return Err("guest initial stack exceeds the 8 MiB safety limit".into());
    }

    let stack_pointer = config
        .stack_top
        .checked_sub(u64::try_from(total_size)?)
        .ok_or("guest stack top is too low for the requested contents")?;
    if stack_pointer % STACK_ALIGNMENT != 0 {
        return Err("internal guest stack alignment calculation failed".into());
    }

    let mut bytes = vec![0_u8; total_size];
    let mut cursor = table_size;
    let mut argv_addresses = Vec::with_capacity(config.argv.len());
    let mut envp_addresses = Vec::with_capacity(config.envp.len());

    for value in &config.argv {
        let address = stack_pointer
            .checked_add(u64::try_from(cursor)?)
            .ok_or("guest argv pointer overflow")?;
        argv_addresses.push(address);
        copy_string(&mut bytes, &mut cursor, value);
    }

    for value in &config.envp {
        let address = stack_pointer
            .checked_add(u64::try_from(cursor)?)
            .ok_or("guest envp pointer overflow")?;
        envp_addresses.push(address);
        copy_string(&mut bytes, &mut cursor, value);
    }

    let execfn_address = stack_pointer
        .checked_add(u64::try_from(cursor)?)
        .ok_or("guest AT_EXECFN pointer overflow")?;
    copy_string(&mut bytes, &mut cursor, &config.execfn);

    let random_address = stack_pointer
        .checked_add(u64::try_from(cursor)?)
        .ok_or("guest AT_RANDOM pointer overflow")?;
    bytes[cursor..cursor + 16].copy_from_slice(&config.random);
    cursor += 16;

    let mut words = Vec::with_capacity(word_count);
    words.push(u64::try_from(config.argv.len())?);
    words.extend(argv_addresses.iter().copied());
    words.push(0);
    words.extend(envp_addresses.iter().copied());
    words.push(0);

    for entry in &config.auxv {
        words.push(entry.kind);
        words.push(entry.value);
    }

    words.extend([
        AT_RANDOM,
        random_address,
        AT_EXECFN,
        execfn_address,
        AT_NULL,
        0,
    ]);

    if words.len() != word_count {
        return Err("internal guest stack pointer table size mismatch".into());
    }

    for (index, word) in words.iter().enumerate() {
        let offset = index * WORD_SIZE;
        bytes[offset..offset + WORD_SIZE].copy_from_slice(&word.to_le_bytes());
    }

    if cursor > bytes.len() {
        return Err("internal guest stack payload exceeds its allocation".into());
    }

    Ok(GuestStack {
        stack_pointer,
        bytes,
        argv_addresses,
        envp_addresses,
        random_address,
        execfn_address,
    })
}

fn copy_string(bytes: &mut [u8], cursor: &mut usize, value: &[u8]) {
    let end = *cursor + value.len();
    bytes[*cursor..end].copy_from_slice(value);
    bytes[end] = 0;
    *cursor = end + 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GuestStackConfig {
        GuestStackConfig {
            stack_top: 0x7fff_ffff_f000,
            argv: vec![b"/bin/demo".to_vec(), b"--version".to_vec()],
            envp: vec![b"PATH=/usr/bin".to_vec()],
            execfn: b"/usr/bin/demo".to_vec(),
            random: [0x5a; 16],
            auxv: vec![
                AuxEntry {
                    kind: 3,
                    value: 0x400040,
                },
                AuxEntry { kind: 4, value: 56 },
                AuxEntry { kind: 5, value: 7 },
                AuxEntry {
                    kind: 6,
                    value: 4096,
                },
            ],
        }
    }

    fn read_word(stack: &GuestStack, index: usize) -> u64 {
        let offset = index * WORD_SIZE;
        u64::from_le_bytes(stack.bytes[offset..offset + WORD_SIZE].try_into().unwrap())
    }

    #[test]
    fn creates_aligned_stack_with_linux_pointer_sections() {
        let stack = build_initial_stack(&config()).unwrap();

        assert_eq!(stack.stack_pointer() % 16, 0);
        assert_eq!(read_word(&stack, 0), 2);
        assert_eq!(read_word(&stack, 1), stack.argv_addresses()[0]);
        assert_eq!(read_word(&stack, 2), stack.argv_addresses()[1]);
        assert_eq!(read_word(&stack, 3), 0);
        assert_eq!(read_word(&stack, 4), stack.envp_addresses()[0]);
        assert_eq!(read_word(&stack, 5), 0);
    }

    #[test]
    fn includes_random_execfn_and_null_auxiliary_entries() {
        let stack = build_initial_stack(&config()).unwrap();
        let aux_start = 6 + 8;

        assert_eq!(read_word(&stack, aux_start), AT_RANDOM);
        assert_eq!(read_word(&stack, aux_start + 1), stack.random_address());
        assert_eq!(read_word(&stack, aux_start + 2), AT_EXECFN);
        assert_eq!(read_word(&stack, aux_start + 3), stack.execfn_address());
        assert_eq!(read_word(&stack, aux_start + 4), AT_NULL);
        assert_eq!(read_word(&stack, aux_start + 5), 0);
    }

    #[test]
    fn stores_strings_execfn_and_random_bytes_at_guest_addresses() {
        let stack = build_initial_stack(&config()).unwrap();
        let argv_offset = (stack.argv_addresses()[0] - stack.stack_pointer()) as usize;

        assert_eq!(
            &stack.bytes()[argv_offset..argv_offset + 10],
            b"/bin/demo\0"
        );

        let execfn_offset = (stack.execfn_address() - stack.stack_pointer()) as usize;
        assert_eq!(
            &stack.bytes()[execfn_offset..execfn_offset + 14],
            b"/usr/bin/demo\0"
        );

        let random_offset = (stack.random_address() - stack.stack_pointer()) as usize;
        assert_eq!(
            &stack.bytes()[random_offset..random_offset + 16],
            &[0x5a; 16]
        );
    }

    #[test]
    fn rejects_unaligned_stack_top() {
        let mut input = config();
        input.stack_top -= 1;

        assert!(build_initial_stack(&input).is_err());
    }

    #[test]
    fn rejects_empty_argv_and_execfn() {
        let mut input = config();
        input.argv.clear();
        assert!(build_initial_stack(&input).is_err());

        let mut input = config();
        input.execfn.clear();
        assert!(build_initial_stack(&input).is_err());
    }

    #[test]
    fn rejects_embedded_nul_and_reserved_auxv_tags() {
        let mut input = config();
        input.argv[0] = b"bad\0argument".to_vec();
        assert!(build_initial_stack(&input).is_err());

        let mut input = config();
        input.auxv.push(AuxEntry {
            kind: AT_RANDOM,
            value: 1,
        });
        assert!(build_initial_stack(&input).is_err());
    }

    #[test]
    fn rejects_stack_size_overflow() {
        let mut input = config();
        input.argv = vec![vec![b'x'; MAX_STACK_SIZE]];

        assert!(build_initial_stack(&input).is_err());
    }
}
