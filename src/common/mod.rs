pub mod account;
pub mod address;
pub mod call;
pub mod error;
pub mod hash;

pub type Word = primitive_types::U256;

pub fn word(s: &str) -> Word {
    let b = decode::<32>(s);
    Word::from_big_endian(&b)
}

pub const fn addr(s: &str) -> address::Address {
    address::Address(decode(s))
}

const fn decode<const N: usize>(s: &str) -> [u8; N] {
    let s = s.as_bytes();
    let mut b = [0u8; N];
    let mut n = s.len();
    let parity = s.len() % 2;

    if s.is_empty() {
        return b;
    }
    let min = if s[0] == b'0' && s.len() > 1 && s[1] == b'x' {
        2
    } else {
        0
    };

    let mut i = N;
    while n > min {
        let c = s[n - 1];
        let c = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("Invalid hex"),
        };

        if n % 2 == parity {
            b[i - 1] = c;
        } else {
            b[i - 1] += c << 4;
            i -= 1;
        }

        n -= 1;
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(s: &str) {
        let hex = hex::decode(s.trim_start_matches("0x")).expect("hex");
        assert_eq!(&decode::<20>(s), &hex[..], "{s}");
    }

    #[test]
    fn test_decode() {
        assert_eq!(
            decode("123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(
            decode("0x123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(
            decode("0123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(
            decode("0x0123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        check("0xc80a141ce8a5b73371043cba5cee40437975bb37");
    }
}
