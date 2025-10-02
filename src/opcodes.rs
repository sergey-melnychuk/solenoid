use once_cell::sync::Lazy;

#[derive(Debug, Clone, Copy)]
pub struct Opcode {
    pub code: u8,
    pub name: &'static str,
    pub n: u8,
}

impl Opcode {
    pub fn new(code: u8, name: &'static str, n: u8) -> Self {
        Self { code, name, n }
    }

    pub fn name(&self) -> String {
        self.name.replace('_', &self.n.to_string())
    }

    pub(crate) fn push_len(&self) -> usize {
        if self.name != "PUSH_" {
            0
        } else {
            self.n as usize
        }
    }
}

static OPCODES: Lazy<[Opcode; 256]> = Lazy::new(|| {
    let mut table = [Opcode::new(0xfe, "undefined", 0); 256];

    // 0s: Stop and Arithmetic Operations
    table[0x00] = Opcode::new(0x00, "STOP", 0);
    table[0x01] = Opcode::new(0x01, "ADD", 0);
    table[0x02] = Opcode::new(0x02, "MUL", 0);
    table[0x03] = Opcode::new(0x03, "SUB", 0);
    table[0x04] = Opcode::new(0x04, "DIV", 0);
    table[0x05] = Opcode::new(0x05, "SDIV", 0);
    table[0x06] = Opcode::new(0x06, "MOD", 0);
    table[0x07] = Opcode::new(0x07, "SMOD", 0);
    table[0x08] = Opcode::new(0x08, "ADDMOD", 0);
    table[0x09] = Opcode::new(0x09, "MULMOD", 0);
    table[0x0a] = Opcode::new(0x0a, "EXP", 0);
    table[0x0b] = Opcode::new(0x0b, "SIGNEXTEND", 0);

    // 10s: Comparison & Bitwise Logic Operations
    table[0x10] = Opcode::new(0x10, "LT", 0);
    table[0x11] = Opcode::new(0x11, "GT", 0);
    table[0x12] = Opcode::new(0x12, "SLT", 0);
    table[0x13] = Opcode::new(0x13, "SGT", 0);
    table[0x14] = Opcode::new(0x14, "EQ", 0);
    table[0x15] = Opcode::new(0x15, "ISZERO", 0);
    table[0x16] = Opcode::new(0x16, "AND", 0);
    table[0x17] = Opcode::new(0x17, "OR", 0);
    table[0x18] = Opcode::new(0x18, "XOR", 0);
    table[0x19] = Opcode::new(0x19, "NOT", 0);
    table[0x1a] = Opcode::new(0x1a, "BYTE", 0);
    table[0x1b] = Opcode::new(0x1b, "SHL", 0);
    table[0x1c] = Opcode::new(0x1c, "SHR", 0);
    table[0x1d] = Opcode::new(0x1d, "SAR", 0);

    // 20s: SHA3
    table[0x20] = Opcode::new(0x20, "SHA3", 0);

    // 30s: Environmental Information
    table[0x30] = Opcode::new(0x30, "ADDRESS", 0);
    table[0x31] = Opcode::new(0x31, "BALANCE", 0);
    table[0x32] = Opcode::new(0x32, "ORIGIN", 0);
    table[0x33] = Opcode::new(0x33, "CALLER", 0);
    table[0x34] = Opcode::new(0x34, "CALLVALUE", 0);
    table[0x35] = Opcode::new(0x35, "CALLDATALOAD", 0);
    table[0x36] = Opcode::new(0x36, "CALLDATASIZE", 0);
    table[0x37] = Opcode::new(0x37, "CALLDATACOPY", 0);
    table[0x38] = Opcode::new(0x38, "CODESIZE", 0);
    table[0x39] = Opcode::new(0x39, "CODECOPY", 0);
    table[0x3a] = Opcode::new(0x3a, "GASPRICE", 0);
    table[0x3b] = Opcode::new(0x3b, "EXTCODESIZE", 0);
    table[0x3c] = Opcode::new(0x3c, "EXTCODECOPY", 0);
    table[0x3d] = Opcode::new(0x3d, "RETURNDATASIZE", 0);
    table[0x3e] = Opcode::new(0x3e, "RETURNDATACOPY", 0);
    table[0x3f] = Opcode::new(0x3f, "EXTCODEHASH", 0);

    // 40s: Block Information
    table[0x40] = Opcode::new(0x40, "BLOCKHASH", 0);
    table[0x41] = Opcode::new(0x41, "COINBASE", 0);
    table[0x42] = Opcode::new(0x42, "TIMESTAMP", 0);
    table[0x43] = Opcode::new(0x43, "NUMBER", 0);
    table[0x44] = Opcode::new(0x44, "DIFFICULTY", 0);
    table[0x45] = Opcode::new(0x45, "GASLIMIT", 0);
    table[0x46] = Opcode::new(0x46, "CHAINID", 0);
    table[0x47] = Opcode::new(0x47, "SELFBALANCE", 0);
    table[0x48] = Opcode::new(0x48, "BASEFEE", 0);
    table[0x49] = Opcode::new(0x49, "BLOBHASH", 0);
    table[0x4a] = Opcode::new(0x4a, "BLOBBASEFEE", 0);

    // 50s: Stack, Memory, Storage and Flow Operations
    table[0x50] = Opcode::new(0x50, "POP", 0);
    table[0x51] = Opcode::new(0x51, "MLOAD", 0);
    table[0x52] = Opcode::new(0x52, "MSTORE", 0);
    table[0x53] = Opcode::new(0x53, "MSTORE8", 0);
    table[0x54] = Opcode::new(0x54, "SLOAD", 0);
    table[0x55] = Opcode::new(0x55, "SSTORE", 0);
    table[0x56] = Opcode::new(0x56, "JUMP", 0);
    table[0x57] = Opcode::new(0x57, "JUMPI", 0);
    table[0x58] = Opcode::new(0x58, "PC", 0);
    table[0x59] = Opcode::new(0x59, "MSIZE", 0);
    table[0x5a] = Opcode::new(0x5a, "GAS", 0);
    table[0x5b] = Opcode::new(0x5b, "JUMPDEST", 0);
    table[0x5c] = Opcode::new(0x5c, "TLOAD", 0);
    table[0x5d] = Opcode::new(0x5d, "TSTORE", 0);
    table[0x5e] = Opcode::new(0x5e, "MCOPY", 0);
    table[0x5f] = Opcode::new(0x5f, "PUSH0", 0);

    // PUSH{1..32} Operations
    for i in 0..32 {
        table[0x60 + i] = Opcode::new(0x60 + i as u8, "PUSH_", i as u8 + 1);
    }

    // DUP{1..16}
    for i in 0..16 {
        table[0x80 + i] = Opcode::new(0x80 + i as u8, "DUP_", i as u8 + 1);
    }

    // SWAP{1..16}
    for i in 0..16 {
        table[0x90 + i] = Opcode::new(0x90 + i as u8, "SWAP_", i as u8 + 1);
    }

    // LOG{0..4}
    for i in 0..5 {
        table[0xa0 + i] = Opcode::new(0xa0 + i as u8, "LOG_", i as u8);
    }

    // System operations
    table[0xf0] = Opcode::new(0xf0, "CREATE", 0);
    table[0xf1] = Opcode::new(0xf1, "CALL", 0);
    table[0xf2] = Opcode::new(0xf2, "CALLCODE", 0);
    table[0xf3] = Opcode::new(0xf3, "RETURN", 0);
    table[0xf4] = Opcode::new(0xf4, "DELEGATECALL", 0);
    table[0xf5] = Opcode::new(0xf5, "CREATE2", 0);
    table[0xfa] = Opcode::new(0xfa, "STATICCALL", 0);
    table[0xfd] = Opcode::new(0xfd, "REVERT", 0);
    table[0xfe] = Opcode::new(0xfe, "INVALID", 0);
    table[0xff] = Opcode::new(0xff, "SELFDESTRUCT", 0);

    table
});

pub fn get_opcode(value: u8) -> Opcode {
    OPCODES[value as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_all_opcodes_covered() {
        for i in 0..0xffu8 {
            if OPCODES[i as usize].code == i {
                assert_ne!(OPCODES[i as usize].name, "undefined");
                continue;
            }
            assert_eq!(OPCODES[i as usize].code, 0xfe);
            assert_eq!(OPCODES[i as usize].name, "undefined");
        }
    }
}
