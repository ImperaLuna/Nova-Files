//! MD5, which the freedesktop thumbnail spec uses to name a thumbnail after
//! its file's URI. Not for anything that needs security.

/// Per-round shift amounts.
const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];
/// Per-round constants: the integer parts of `abs(sin(i + 1)) * 2^32`.
const SINES: [u32; 64] = [
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
/// The digest is this many 32-bit words.
const STATE_WORDS: usize = 4;
const INITIAL: [u32; STATE_WORDS] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
const BLOCK: usize = 64;
const WORD: usize = 4;
/// The message is padded to this many bytes past a block boundary, then
/// its bit length fills the last eight.
const LENGTH_AT: usize = 56;
const PAD_START: u8 = 0x80;
const BITS_PER_BYTE: u64 = 8;
const ROUND: usize = 16;
const HEX: &[u8; 16] = b"0123456789abcdef";
const HIGH_NIBBLE: u32 = 4;
const LOW_NIBBLE_MASK: u8 = 0x0f;
/// Message-word index steps (multiply, then add) for rounds two to four.
const STEP_TWO: (usize, usize) = (5, 1);
const STEP_THREE: (usize, usize) = (3, 5);
const STEP_FOUR: (usize, usize) = (7, 0);
const ROUND_ONE: usize = 0;
const ROUND_TWO: usize = 1;
const ROUND_THREE: usize = 2;
/// Two hex digits per digest byte.
const DIGEST_HEX_LENGTH: usize = STATE_WORDS * WORD * 2;

/// The digest of `bytes` as 32 lowercase hex digits.
pub fn hex(bytes: &[u8]) -> String {
    let mut state = INITIAL;
    let bit_length = (bytes.len() as u64).wrapping_mul(BITS_PER_BYTE);
    let mut message = bytes.to_vec();
    message.push(PAD_START);
    while message.len() % BLOCK != LENGTH_AT {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_le_bytes());
    for block in message.as_chunks::<BLOCK>().0 {
        compress(&mut state, block);
    }
    let mut out = String::with_capacity(DIGEST_HEX_LENGTH);
    for byte in state.iter().flat_map(|word| word.to_le_bytes()) {
        out.push(char::from(HEX[usize::from(byte >> HIGH_NIBBLE)]));
        out.push(char::from(HEX[usize::from(byte & LOW_NIBBLE_MASK)]));
    }
    out
}

fn compress(state: &mut [u32; STATE_WORDS], block: &[u8; BLOCK]) {
    let mut words = [0_u32; ROUND];
    for (word, bytes) in words.iter_mut().zip(block.as_chunks::<WORD>().0) {
        *word = u32::from_le_bytes(*bytes);
    }
    let [mut a, mut b, mut c, mut d] = *state;
    for i in 0..BLOCK {
        let round = i / ROUND;
        let (mixed, index) = match round {
            ROUND_ONE => ((b & c) | (!b & d), i),
            ROUND_TWO => ((d & b) | (!d & c), step(i, STEP_TWO)),
            ROUND_THREE => (b ^ c ^ d, step(i, STEP_THREE)),
            _ => (c ^ (b | !d), step(i, STEP_FOUR)),
        };
        let sum = mixed
            .wrapping_add(a)
            .wrapping_add(SINES[i])
            .wrapping_add(words[index]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(sum.rotate_left(SHIFTS[i]));
    }
    for (value, add) in state.iter_mut().zip([a, b, c, d]) {
        *value = value.wrapping_add(add);
    }
}

const fn step(i: usize, (times, plus): (usize, usize)) -> usize {
    (times * i + plus) % ROUND
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_rfc_1321_test_suite() {
        assert_eq!(hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn names_a_thumbnail_as_the_spec_example_does() {
        // From the freedesktop thumbnail spec.
        assert_eq!(
            hex(b"file:///home/jens/photos/me.png"),
            "c6ee772d9e49320e97ec29a7eb5b1697"
        );
    }
}
