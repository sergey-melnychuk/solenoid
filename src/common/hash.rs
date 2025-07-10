pub fn sha3(input: &[u8]) -> [u8; 32] {
    use tiny_keccak::Hasher;
    let mut sha3 = tiny_keccak::Sha3::v256();
    let mut ret = [0u8; 32];
    sha3.update(input);
    sha3.finalize(&mut ret);
    ret
}
