use primitive_types::U256;

#[derive(Clone, Default, Debug)]
pub struct Account {
    pub balance: U256,
    pub nonce: U256,
    pub code: U256,
    pub root: U256,
}
