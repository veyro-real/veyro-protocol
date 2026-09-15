use core::mem::size_of;

const CREATE_POLICY: u8 = 1;
const REVOKE_POLICY: u8 = 2;
const CHECK_SPEND: u8 = 3;
const VERSION: u8 = 1;
const OK: u64 = 0;
const INVALID_ARGUMENT: u64 = 2u64 << 32;
const INVALID_INSTRUCTION_DATA: u64 = 3u64 << 32;
const INVALID_ACCOUNT_DATA: u64 = 4u64 << 32;
const ACCOUNT_DATA_TOO_SMALL: u64 = 5u64 << 32;
const INSUFFICIENT_FUNDS: u64 = 6u64 << 32;
const MISSING_REQUIRED_SIGNATURES: u64 = 8u64 << 32;
const NOT_ENOUGH_ACCOUNT_KEYS: u64 = 11u64 << 32;
const ARITHMETIC_OVERFLOW: u64 = 24u64 << 32;
const NON_DUP_MARKER: u8 = u8::MAX;
const POLICY_LEN: usize = 154;
const MAX_PERMITTED_DATA_INCREASE: usize = 10 * 1024;
const BPF_ALIGN_OF_U128: usize = 8;

struct RawAccount {
    is_signer: bool,
    is_writable: bool,
    key: *const u8,
    data: *mut u8,
    data_len: usize,
}

#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
    let (accounts, count, data) = match deserialize(input) {
        Ok(value) => value,
        Err(error) => return error,
    };
    if data.is_empty() {
        return INVALID_INSTRUCTION_DATA;
    }
    match data[0] {
        CREATE_POLICY => create_policy(accounts, count, data),
        REVOKE_POLICY => revoke_policy(accounts, count),
        CHECK_SPEND => check_spend(accounts, count, data),
        _ => INVALID_INSTRUCTION_DATA,
    }
}

unsafe fn deserialize(input: *mut u8) -> Result<([RawAccount; 2], usize, &'static [u8]), u64> {
    let mut offset = 0usize;
    let count = read_u64_at(input, &mut offset) as usize;
    if count > 2 {
        return Err(INVALID_ARGUMENT);
    }
    let mut accounts = [
        RawAccount { is_signer: false, is_writable: false, key: core::ptr::null(), data: core::ptr::null_mut(), data_len: 0 },
        RawAccount { is_signer: false, is_writable: false, key: core::ptr::null(), data: core::ptr::null_mut(), data_len: 0 },
    ];
    for slot in accounts.iter_mut().take(count) {
        let dup = *input.add(offset);
        offset += 1;
        if dup != NON_DUP_MARKER {
            return Err(INVALID_ARGUMENT);
        }
        slot.is_signer = *input.add(offset) != 0;
        offset += 1;
        slot.is_writable = *input.add(offset) != 0;
        offset += 1;
        offset += 1 + size_of::<u32>();
        slot.key = input.add(offset);
        offset += 32;
        offset += 32;
        offset += 8;
        slot.data_len = read_u64_at(input, &mut offset) as usize;
        slot.data = input.add(offset);
        offset += slot.data_len + MAX_PERMITTED_DATA_INCREASE + 8;
        let align = (BPF_ALIGN_OF_U128 - (offset % BPF_ALIGN_OF_U128)) % BPF_ALIGN_OF_U128;
        offset += align;
    }
    let ix_len = read_u64_at(input, &mut offset) as usize;
    let ix = core::slice::from_raw_parts(input.add(offset), ix_len);
    Ok((accounts, count, ix))
}

fn create_policy(accounts: [RawAccount; 2], count: usize, data: &[u8]) -> u64 {
    if count < 2 {
        return NOT_ENOUGH_ACCOUNT_KEYS;
    }
    let owner = &accounts[0];
    let policy = &accounts[1];
    if !owner.is_signer || !policy.is_writable {
        return MISSING_REQUIRED_SIGNATURES;
    }
    if data.len() != 113 {
        return INVALID_INSTRUCTION_DATA;
    }
    if policy.data_len < POLICY_LEN {
        return ACCOUNT_DATA_TOO_SMALL;
    }
    unsafe { write_policy(policy.data, owner.key, data); }
    OK
}

fn revoke_policy(accounts: [RawAccount; 2], count: usize) -> u64 {
    if count < 2 {
        return NOT_ENOUGH_ACCOUNT_KEYS;
    }
    if !accounts[0].is_signer || !accounts[1].is_writable {
        return MISSING_REQUIRED_SIGNATURES;
    }
    unsafe {
        if accounts[1].data_len < POLICY_LEN || *accounts[1].data != VERSION || !same(accounts[1].data.add(2), accounts[0].key) {
            return INVALID_ACCOUNT_DATA;
        }
        *accounts[1].data.add(1) = 0;
    }
    OK
}

fn check_spend(accounts: [RawAccount; 2], count: usize, data: &[u8]) -> u64 {
    if count < 2 {
        return NOT_ENOUGH_ACCOUNT_KEYS;
    }
    if !accounts[0].is_signer || data.len() != 65 {
        return if data.len() == 65 { MISSING_REQUIRED_SIGNATURES } else { INVALID_INSTRUCTION_DATA };
    }
    unsafe {
        let out = accounts[1].data;
        if accounts[1].data_len < POLICY_LEN || *out != VERSION || *out.add(1) != 1 || !same(out.add(34), accounts[0].key) {
            return INVALID_ACCOUNT_DATA;
        }
        let amount = le_u64(data.as_ptr().add(1));
        let spent = le_u64(out.add(82));
        if amount == 0 || amount > le_u64(out.add(66)) || le_u64(data.as_ptr().add(9)) > le_u64(out.add(90)) {
            return INVALID_ARGUMENT;
        }
        let next = match spent.checked_add(amount) {
            Some(value) => value,
            None => return ARITHMETIC_OVERFLOW,
        };
        if next > le_u64(out.add(74)) {
            return INSUFFICIENT_FUNDS;
        }
        if !same(out.add(106), data.as_ptr().add(17)) || !same16(out.add(138), data.as_ptr().add(49)) {
            return INVALID_ARGUMENT;
        }
        let nonce = match le_u64(out.add(98)).checked_add(1) {
            Some(value) => value,
            None => return ARITHMETIC_OVERFLOW,
        };
        write_u64(out.add(82), next);
        write_u64(out.add(98), nonce);
    }
    OK
}

unsafe fn read_u64_at(input: *mut u8, offset: &mut usize) -> u64 {
    let value = *(input.add(*offset) as *const u64);
    *offset += 8;
    value
}

unsafe fn write_policy(out: *mut u8, owner: *const u8, data: &[u8]) {
    *out = VERSION;
    *out.add(1) = 1;
    copy32(out.add(2), owner);
    copy32(out.add(34), data.as_ptr().add(1));
    copy8(out.add(66), data.as_ptr().add(33));
    copy8(out.add(74), data.as_ptr().add(41));
    write_u64(out.add(82), 0);
    copy8(out.add(90), data.as_ptr().add(49));
    copy8(out.add(98), data.as_ptr().add(57));
    copy32(out.add(106), data.as_ptr().add(65));
    for i in 0..16 {
        *out.add(138 + i) = *data.as_ptr().add(97 + i);
    }
}

unsafe fn le_u64(data: *const u8) -> u64 {
    *(data as *const u64)
}

unsafe fn write_u64(out: *mut u8, value: u64) {
    *(out as *mut u64) = value;
}

unsafe fn copy8(out: *mut u8, input: *const u8) {
    *(out as *mut u64) = *(input as *const u64);
}

unsafe fn copy32(out: *mut u8, input: *const u8) {
    copy8(out, input);
    copy8(out.add(8), input.add(8));
    copy8(out.add(16), input.add(16));
    copy8(out.add(24), input.add(24));
}

unsafe fn same(left: *const u8, right: *const u8) -> bool {
    *(left as *const u64) == *(right as *const u64)
        && *(left.add(8) as *const u64) == *(right.add(8) as *const u64)
        && *(left.add(16) as *const u64) == *(right.add(16) as *const u64)
        && *(left.add(24) as *const u64) == *(right.add(24) as *const u64)
}

unsafe fn same16(left: *const u8, right: *const u8) -> bool {
    *(left as *const u64) == *(right as *const u64)
        && *(left.add(8) as *const u64) == *(right.add(8) as *const u64)
}
