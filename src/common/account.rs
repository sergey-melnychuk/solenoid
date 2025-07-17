use crate::common::Word;

#[derive(Clone, Default, Debug)]
pub struct Account {
    pub balance: Word,
    pub nonce: Word,
    pub code_hash: Word,
    pub root: Word,
}
