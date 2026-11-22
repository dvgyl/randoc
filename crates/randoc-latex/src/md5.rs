//! The MD5 digest of RFC 1321, for the `pdf-trailer-id` template variable.
//! The digest only gives a reproducible identifier; it has no security use.

/// The shift amounts of the 64 steps.
const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// The constants of the 64 steps: the integer part of `abs(sin(i + 1)) * 2^32`.
const CONSTANTS: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// An MD5 digest that takes its input in parts, so that the caller does
/// not copy the input into one buffer.
pub(crate) struct Md5 {
    state: [u32; 4],
    /// The bytes after the last full block of 64 bytes.
    pending: Vec<u8>,
    length: u64,
}

impl Md5 {
    pub(crate) fn new() -> Self {
        Md5 {
            state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            pending: Vec::with_capacity(64),
            length: 0,
        }
    }

    /// Adds bytes to the input.
    pub(crate) fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if !self.pending.is_empty() {
            let taken = data.len().min(64 - self.pending.len());
            self.pending.extend_from_slice(&data[..taken]);
            data = &data[taken..];
            if self.pending.len() < 64 {
                return;
            }
            let block = std::mem::take(&mut self.pending);
            self.compress(&block);
            self.pending = block;
            self.pending.clear();
        }
        let (blocks, rest) = data.as_chunks::<64>();
        for block in blocks {
            self.compress(block);
        }
        self.pending.extend_from_slice(rest);
    }

    /// Returns the digest as 32 lowercase hexadecimal digits.
    pub(crate) fn finish(mut self) -> String {
        let bit_length = self.length.wrapping_mul(8);
        let mut tail = std::mem::take(&mut self.pending);
        tail.push(0x80);
        while tail.len() % 64 != 56 {
            tail.push(0);
        }
        tail.extend_from_slice(&bit_length.to_le_bytes());
        for block in tail.as_chunks::<64>().0 {
            self.compress(block);
        }
        self.state
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn compress(&mut self, block: &[u8]) {
        let mut words = [0u32; 16];
        for (word, bytes) in words.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*bytes);
        }
        let [mut a, mut b, mut c, mut d] = self.state;
        for step in 0..64 {
            let (mixed, index) = match step / 16 {
                0 => ((b & c) | (!b & d), step),
                1 => ((d & b) | (!d & c), (5 * step + 1) % 16),
                2 => (b ^ c ^ d, (3 * step + 5) % 16),
                _ => (c ^ (b | !d), (7 * step) % 16),
            };
            let sum = a
                .wrapping_add(mixed)
                .wrapping_add(CONSTANTS[step])
                .wrapping_add(words[index]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(sum.rotate_left(SHIFTS[step]));
        }
        for (value, added) in self.state.iter_mut().zip([a, b, c, d]) {
            *value = value.wrapping_add(added);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Md5;

    fn hex_digest(data: &[u8]) -> String {
        let mut md5 = Md5::new();
        md5.update(data);
        md5.finish()
    }

    #[test]
    fn digests_equal_the_test_suite_of_rfc_1321() {
        let cases = [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
            ("1700000000s", "dcddef5362184a0ec6d955321d3898c4"),
        ];
        for (input, digest) in cases {
            assert_eq!(hex_digest(input.as_bytes()), digest, "{input:?}");
            // The same input in parts of each size gives the same digest.
            for size in [1, 3, 63, 64, 65] {
                let mut md5 = Md5::new();
                for part in input.as_bytes().chunks(size) {
                    md5.update(part);
                }
                assert_eq!(md5.finish(), digest, "{input:?} in parts of {size}");
            }
        }
    }
}
