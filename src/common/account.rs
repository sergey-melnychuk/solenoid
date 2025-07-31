use crate::common::word::Word;

#[derive(Clone, Debug, Default)]
pub struct Account {
    pub value: Word,
    pub nonce: Word,
    pub root: Word,
}
