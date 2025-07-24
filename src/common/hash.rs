use crate::common::decode;

pub fn keccak256(input: &[u8]) -> [u8; 32] {
    use tiny_keccak::Hasher;
    let mut sha3 = tiny_keccak::Keccak::v256();
    let mut ret = [0u8; 32];
    sha3.update(input);
    sha3.finalize(&mut ret);
    ret
}

pub const fn empty() -> [u8; 32] {
    decode("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input_hash() {
        let hash = keccak256(&[]);
        assert_eq!(hex::encode(&hash), hex::encode(&empty()));
    }
}
