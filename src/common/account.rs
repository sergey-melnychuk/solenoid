use crate::common::Word;

#[derive(Clone, Debug, Default)]
pub struct Account {
    pub balance: Word,
    pub nonce: Word,
    pub code: Word,
    pub root: Word,
}
