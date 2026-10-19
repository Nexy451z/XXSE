//! CRC-32 (IEEE 802.3, reflected, poly 0xEDB88320) helpers.
//!
//! The Switch save header stores crc32(file_name) XOR 0xFFFFFFFF, i.e. the
//! standard CRC-32 **without** the final one's-complement, which is exactly
//! what name_hash() returns.

const TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// Standard CRC-32 - the same value Python's binascii.crc32 returns.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// CRC-32 with the final one's-complement omitted.
pub fn crc32_no_final(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

/// The value stored at offset 0x14 of the 36 byte Switch save header.
///
/// * name_hash(b"system")        == 0x36B2EE74
/// * name_hash(b"system_backup") == 0xD0819B92
pub fn name_hash(name: &[u8]) -> u32 {
    crc32_no_final(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn save_header_hashes_match_real_files() {
        assert_eq!(name_hash(b"system"), 0x36B2_EE74);
        assert_eq!(name_hash(b"system_backup"), 0xD081_9B92);
        assert_eq!(crc32(b"system") ^ 0xFFFF_FFFF, 0x36B2_EE74);
    }
}
