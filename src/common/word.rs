use std::ops::{BitAnd, BitOr, BitXor, Shl, Shr};

use k256::elliptic_curve::bigint::Encoding;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::common::decode;

type U256 = primitive_types::U256;

#[derive(Default, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Word(U256);

impl Word {
    pub fn mul_modulo(&self, that: &Word, modulo: &Word) -> Word {
        let res = self.0.full_mul(that.0) % modulo.0;
        Word(U256::from_big_endian(&res.to_big_endian()[32..]))
    }

    pub fn add_modulo(&self, that: &Word, modulo: &Word) -> Word {
        let a = k256::U256::from_be_slice(&self.into_bytes());
        let b = k256::U256::from_be_slice(&that.into_bytes());
        let m = k256::U256::from_be_slice(&modulo.into_bytes());
        let r = (&a).add_mod(&b, &m);
        Self::from_bytes(&r.to_be_bytes())
    }
}

impl std::fmt::Debug for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::LowerHex::fmt(&self.0, f)
    }
}

impl std::fmt::Display for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::LowerHex::fmt(&self.0, f)
    }
}

impl std::fmt::LowerHex for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::LowerHex::fmt(&self.0, f)
    }
}

impl std::fmt::UpperHex for Word {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::UpperHex::fmt(&self.0, f)
    }
}

impl Word {
    pub fn into_bytes(&self) -> [u8; 32] {
        self.0.to_big_endian()
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn as_u128(&self) -> u128 {
        self.0.as_u128()
    }

    pub fn as_u64(&self) -> u64 {
        self.0.as_u64()
    }

    pub fn as_i64(&self) -> i64 {
        self.0.as_u64() as i64
    }

    pub fn as_usize(&self) -> usize {
        // WASM32 safe: Check if value fits in usize
        #[cfg(target_pointer_width = "32")]
        {
            if self.0 > primitive_types::U256::from(u32::MAX) {
                // For WASM32, cap at u32::MAX to avoid overflow
                // This is reasonable since WASM32 can't address more than 4GB anyway
                u32::MAX as usize
            } else {
                self.0.as_u32() as usize
            }
        }
        #[cfg(not(target_pointer_width = "32"))]
        {
            self.0.as_usize()
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let word = primitive_types::U256::from_big_endian(bytes);
        Self(word)
    }

    pub fn zero() -> Self {
        Self(primitive_types::U256::zero())
    }

    pub fn one() -> Self {
        Self(primitive_types::U256::one())
    }

    pub fn max() -> Self {
        Self(primitive_types::U256::max_value())
    }

    pub fn bit(&self, index: usize) -> bool {
        self.0.bit(index)
    }

    pub fn pow(&self, exp: Self) -> Self {
        let (ret, _) = self.0.overflowing_pow(exp.0);
        Self(ret)
    }

    pub fn saturating_sub(&self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    pub fn overflowing_add(&self, rhs: Self) -> (Self, bool) {
        let (word, flag) = self.0.overflowing_add(rhs.0);
        (Self(word), flag)
    }

    pub fn overflowing_mul(&self, rhs: Self) -> (Self, bool) {
        let (word, flag) = self.0.overflowing_mul(rhs.0);
        (Self(word), flag)
    }

    pub fn overflowing_sub(&self, rhs: Self) -> (Self, bool) {
        let (word, flag) = self.0.overflowing_sub(rhs.0);
        (Self(word), flag)
    }

    pub fn from_hex(hex: &str) -> eyre::Result<Self> {
        let hex = hex.trim_start_matches("0x");
        let word = primitive_types::U256::from_str_radix(hex, 16);
        Ok(Self(
            word.map_err(|_| eyre::eyre!("Invalid U256: '{hex}'."))?,
        ))
    }
}

impl From<u8> for Word {
    fn from(value: u8) -> Self {
        Self(primitive_types::U256::from(value))
    }
}

impl From<i32> for Word {
    fn from(value: i32) -> Self {
        Self(primitive_types::U256::from(value))
    }
}

impl From<i64> for Word {
    fn from(value: i64) -> Self {
        Self(primitive_types::U256::from(value))
    }
}

impl From<u64> for Word {
    fn from(value: u64) -> Self {
        Self(primitive_types::U256::from(value))
    }
}

impl From<usize> for Word {
    fn from(value: usize) -> Self {
        Self(primitive_types::U256::from(value))
    }
}

impl From<u128> for Word {
    fn from(value: u128) -> Self {
        Self(primitive_types::U256::from(value))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "testkit")]
impl From<Word> for evm_tracer::alloy_primitives::U256 {
    fn from(value: Word) -> Self {
        Self::from_be_slice(&value.0.to_big_endian())
    }
}

impl std::ops::Sub<Word> for Word {
    type Output = Word;

    fn sub(self, rhs: Word) -> Self::Output {
        Word(self.0 - rhs.0)
    }
}

impl std::ops::SubAssign<Word> for Word {
    fn sub_assign(&mut self, rhs: Word) {
        self.0 -= rhs.0;
    }
}

impl std::ops::Add<Word> for Word {
    type Output = Word;

    fn add(self, rhs: Word) -> Self::Output {
        Word(self.0 + rhs.0)
    }
}

impl std::ops::AddAssign<Word> for Word {
    fn add_assign(&mut self, rhs: Word) {
        self.0 += rhs.0;
    }
}

impl std::ops::Mul<Word> for Word {
    type Output = Word;

    fn mul(self, rhs: Word) -> Self::Output {
        Word(self.0 * rhs.0)
    }
}

impl std::ops::MulAssign<Word> for Word {
    fn mul_assign(&mut self, rhs: Word) {
        self.0 *= rhs.0;
    }
}

impl std::ops::Div<Word> for Word {
    type Output = Word;

    fn div(self, rhs: Word) -> Self::Output {
        Word(self.0 / rhs.0)
    }
}

impl std::ops::DivAssign<Word> for Word {
    fn div_assign(&mut self, rhs: Word) {
        self.0 /= rhs.0;
    }
}

impl std::ops::Rem<Word> for Word {
    type Output = Word;

    fn rem(self, rhs: Word) -> Self::Output {
        Word(self.0 % rhs.0)
    }
}

impl std::ops::RemAssign<Word> for Word {
    fn rem_assign(&mut self, rhs: Word) {
        self.0 %= rhs.0;
    }
}

impl BitAnd for Word {
    type Output = Word;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitOr for Word {
    type Output = Word;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitXor for Word {
    type Output = Word;

    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}

impl std::ops::Not for Word {
    type Output = Word;

    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

impl Shl<usize> for Word {
    type Output = Word;

    fn shl(self, rhs: usize) -> Self::Output {
        Self(self.0 << rhs)
    }
}

impl Shr<usize> for Word {
    type Output = Word;

    fn shr(self, rhs: usize) -> Self::Output {
        Self(self.0 >> rhs)
    }
}

impl Serialize for Word {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex: String = hex::encode(self.0.to_big_endian())
            .chars()
            .skip_while(|c| c == &'0')
            .collect();
        let hex = format!("0x{hex}");
        serializer.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for Word {
    fn deserialize<D>(deserializer: D) -> Result<Word, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex: String = Deserialize::deserialize(deserializer)?;
        let word = word(hex.trim_start_matches("0x"));
        Ok(word)
    }
}

pub fn word(s: &str) -> Word {
    let b = decode::<32>(s);
    Word::from_bytes(&b)
}

pub fn decode_error_string(ret: &[u8]) -> Option<String> {
    if ret.len() < 4 + 32 + 32 {
        return None;
    }
    let _selector = &ret[0..4];
    let offset = Word::from_bytes(&ret[4..4 + 32]);
    if offset > Word::from(u64::MAX) {
        return None;
    }
    let offset = 4 + 32 + offset.as_usize();
    let size = Word::from_bytes(&ret[4 + 32..4 + 32 + 32]);
    if size > Word::from(u64::MAX) {
        return None;
    }
    let size = size.as_usize();
    if ret.len() < offset + size {
        return None;
    }
    let data = &ret[offset..offset + size];
    String::from_utf8(data.to_vec()).ok()
}
