use thiserror::Error;

#[derive(Error, Debug)]
pub enum EofError {
    #[error("Invalid EOF magic byte: expected 0xEF, got {0:#02x}")]
    InvalidMagic(u8),
    #[error("Invalid EOF version: {0}")]
    InvalidVersion(u8),
    #[error("Invalid section kind: {0:#02x}")]
    InvalidSectionKind(u8),
    #[error("Missing section terminator")]
    MissingTerminator,
    #[error("Incomplete section header")]
    IncompleteSectionHeader,
    #[error("Invalid container: {0}")]
    InvalidContainer(String),
    #[error("Type section size must be divisible by 4, got {0}")]
    InvalidTypeSize(usize),
    #[error("Code section count mismatch: expected {expected}, got {got}")]
    CodeCountMismatch { expected: usize, got: usize },
    #[error("Container too large: {0} bytes")]
    ContainerTooLarge(usize),
    #[error("No sections found")]
    NoSections,
    #[error("Invalid bytecode offset")]
    InvalidOffset,
    #[error("Prohibited opcode in EOF: {0:#02x}")]
    ProhibitedOpcode(u8),
}

const EOF_MAGIC: u8 = 0xEF;
const EOF_VERSION_1: u8 = 0x01;
const SECTION_TERMINATOR: u8 = 0x00;

const SECTION_KIND_TYPE: u8 = 0x01;
const SECTION_KIND_CODE: u8 = 0x02;
const SECTION_KIND_CONTAINER: u8 = 0x03;
const SECTION_KIND_DATA: u8 = 0x04;

const MAX_INITCODE_SIZE: usize = 49152; // 48 KB

/// Type metadata for a single code section
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeMetadata {
    pub inputs: u8,
    pub outputs: u8,
    pub max_stack_height: u16,
}

impl TypeMetadata {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EofError> {
        if bytes.len() != 4 {
            return Err(EofError::InvalidContainer(
                "Type metadata must be 4 bytes".to_string(),
            ));
        }
        Ok(Self {
            inputs: bytes[0],
            outputs: bytes[1],
            max_stack_height: u16::from_be_bytes([bytes[2], bytes[3]]),
        })
    }
}

/// EOF Section information
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EofSection {
    Type(Vec<TypeMetadata>),
    Code(Vec<Vec<u8>>),
    Container(Vec<Vec<u8>>),
    Data(Vec<u8>),
}

/// EOF v1 Container
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EofContainer {
    pub version: u8,
    pub types: Vec<TypeMetadata>,
    pub code_sections: Vec<Vec<u8>>,
    pub container_sections: Vec<Vec<u8>>,
    pub data: Vec<u8>,
}

impl EofContainer {
    /// Parse EOF container from bytecode
    pub fn parse(bytecode: &[u8]) -> Result<Self, EofError> {
        if bytecode.is_empty() {
            return Err(EofError::InvalidContainer("Empty bytecode".to_string()));
        }

        // Check magic byte
        if bytecode[0] != EOF_MAGIC {
            return Err(EofError::InvalidMagic(bytecode[0]));
        }

        if bytecode.len() < 3 {
            return Err(EofError::InvalidContainer(
                "Bytecode too short for EOF header".to_string(),
            ));
        }

        // Check version
        let version = bytecode[1];
        if version != EOF_VERSION_1 {
            return Err(EofError::InvalidVersion(version));
        }

        // Parse section headers
        let mut pos = 2;
        let mut type_size: Option<usize> = None;
        let mut code_sizes: Vec<usize> = Vec::new();
        let mut container_sizes: Vec<usize> = Vec::new();
        let mut data_size: Option<usize> = None;
        let mut section_count = 0;

        while pos < bytecode.len() {
            let kind = bytecode[pos];

            if kind == SECTION_TERMINATOR {
                pos += 1;
                break;
            }

            if pos + 2 >= bytecode.len() {
                return Err(EofError::IncompleteSectionHeader);
            }

            let size = u16::from_be_bytes([bytecode[pos + 1], bytecode[pos + 2]]) as usize;

            match kind {
                SECTION_KIND_TYPE => {
                    if type_size.is_some() {
                        return Err(EofError::InvalidContainer(
                            "Multiple type sections".to_string(),
                        ));
                    }
                    type_size = Some(size);
                    pos += 3;
                    section_count += 1;
                }
                SECTION_KIND_CODE => {
                    // Code section header contains number of code sections
                    if pos + 2 >= bytecode.len() {
                        return Err(EofError::IncompleteSectionHeader);
                    }
                    let num_code_sections = size;
                    pos += 3;

                    // Read sizes for each code section
                    for _ in 0..num_code_sections {
                        if pos + 1 >= bytecode.len() {
                            return Err(EofError::IncompleteSectionHeader);
                        }
                        let code_size =
                            u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]) as usize;
                        code_sizes.push(code_size);
                        pos += 2;
                    }
                    section_count += 1;
                }
                SECTION_KIND_CONTAINER => {
                    let num_containers = size;
                    pos += 3;

                    for _ in 0..num_containers {
                        if pos + 1 >= bytecode.len() {
                            return Err(EofError::IncompleteSectionHeader);
                        }
                        let container_size =
                            u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]) as usize;
                        container_sizes.push(container_size);
                        pos += 2;
                    }
                    section_count += 1;
                }
                SECTION_KIND_DATA => {
                    if data_size.is_some() {
                        return Err(EofError::InvalidContainer(
                            "Multiple data sections".to_string(),
                        ));
                    }
                    data_size = Some(size);
                    pos += 3;
                    section_count += 1;
                }
                _ => return Err(EofError::InvalidSectionKind(kind)),
            }
        }

        if section_count == 0 {
            return Err(EofError::NoSections);
        }

        // Validate type section
        let type_size = type_size.unwrap_or(0);
        if type_size % 4 != 0 {
            return Err(EofError::InvalidTypeSize(type_size));
        }

        let num_type_entries = type_size / 4;
        if num_type_entries != code_sizes.len() {
            return Err(EofError::CodeCountMismatch {
                expected: num_type_entries,
                got: code_sizes.len(),
            });
        }

        // Check container size limit
        if bytecode.len() > MAX_INITCODE_SIZE {
            return Err(EofError::ContainerTooLarge(bytecode.len()));
        }

        // Parse type section
        let mut types = Vec::new();
        if type_size > 0 {
            if pos + type_size > bytecode.len() {
                return Err(EofError::InvalidOffset);
            }
            for i in 0..num_type_entries {
                let offset = pos + i * 4;
                let metadata = TypeMetadata::from_bytes(&bytecode[offset..offset + 4])?;
                types.push(metadata);
            }
            pos += type_size;
        }

        // Parse code sections
        let mut code_sections = Vec::new();
        for &code_size in &code_sizes {
            if pos + code_size > bytecode.len() {
                return Err(EofError::InvalidOffset);
            }
            code_sections.push(bytecode[pos..pos + code_size].to_vec());
            pos += code_size;
        }

        // Parse container sections
        let mut container_sections = Vec::new();
        for &container_size in &container_sizes {
            if pos + container_size > bytecode.len() {
                return Err(EofError::InvalidOffset);
            }
            container_sections.push(bytecode[pos..pos + container_size].to_vec());
            pos += container_size;
        }

        // Parse data section
        let data = if let Some(data_size) = data_size {
            if pos + data_size > bytecode.len() {
                return Err(EofError::InvalidOffset);
            }
            bytecode[pos..pos + data_size].to_vec()
        } else {
            // Data section is optional, but if not specified, take remaining bytes
            if pos < bytecode.len() {
                bytecode[pos..].to_vec()
            } else {
                Vec::new()
            }
        };

        Ok(EofContainer {
            version,
            types,
            code_sections,
            container_sections,
            data,
        })
    }

    /// Check if an opcode is prohibited in EOF
    pub fn is_prohibited_opcode(opcode: u8) -> bool {
        matches!(
            opcode,
            0x38 | // CODESIZE
            0x39 | // CODECOPY
            0x3b | // EXTCODESIZE
            0x3c | // EXTCODECOPY
            0x3f | // EXTCODEHASH
            0x56 | // JUMP
            0x57 | // JUMPI
            0x58 | // PC
            0x5a | // GAS
            0xf0 | // CREATE
            0xf1 | // CALL
            0xf2 | // CALLCODE
            0xf5 | // CREATE2
            0xff   // SELFDESTRUCT
        )
    }

    /// Validate EOF container according to EIP-3670
    pub fn validate(&self) -> Result<(), EofError> {
        // Must have at least one code section
        if self.code_sections.is_empty() {
            return Err(EofError::InvalidContainer(
                "Must have at least one code section".to_string(),
            ));
        }

        // Type metadata count must match code section count
        if self.types.len() != self.code_sections.len() {
            return Err(EofError::CodeCountMismatch {
                expected: self.types.len(),
                got: self.code_sections.len(),
            });
        }

        // Validate each code section
        for code in &self.code_sections {
            // Code must not be empty
            if code.is_empty() {
                return Err(EofError::InvalidContainer(
                    "Code section cannot be empty".to_string(),
                ));
            }

            // Check for prohibited opcodes
            let mut pos = 0;
            while pos < code.len() {
                let opcode = code[pos];

                if Self::is_prohibited_opcode(opcode) {
                    return Err(EofError::ProhibitedOpcode(opcode));
                }

                // Skip over immediate bytes for PUSH opcodes
                if (0x60..=0x7f).contains(&opcode) {
                    let push_size = (opcode - 0x5f) as usize;
                    pos += push_size;
                }

                pos += 1;
            }
        }

        Ok(())
    }

    /// Check if bytecode is EOF format
    pub fn is_eof(bytecode: &[u8]) -> bool {
        !bytecode.is_empty() && bytecode[0] == EOF_MAGIC
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_eof() {
        assert!(EofContainer::is_eof(&[0xef, 0x01, 0x00]));
        assert!(!EofContainer::is_eof(&[0x60, 0x00]));
        assert!(!EofContainer::is_eof(&[]));
    }

    #[test]
    fn test_parse_minimal_eof() {
        // EF 01 00 (magic, version, terminator, no sections)
        let result = EofContainer::parse(&[0xef, 0x01, 0x00]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_transaction_86_bytecode() {
        // The actual bytecode from transaction 86
        let bytecode = hex::decode("ef0100630505d8cfafcf56721deb557fdcfdb7dd23810b").unwrap();
        let result = EofContainer::parse(&bytecode);

        match result {
            Ok(container) => {
                println!("Parsed container: {:#?}", container);
            }
            Err(e) => {
                println!("Parse error: {}", e);
            }
        }
    }

    #[test]
    fn test_prohibited_opcodes() {
        assert!(EofContainer::is_prohibited_opcode(0x38)); // CODESIZE
        assert!(EofContainer::is_prohibited_opcode(0x56)); // JUMP
        assert!(EofContainer::is_prohibited_opcode(0xf0)); // CREATE
        assert!(!EofContainer::is_prohibited_opcode(0x00)); // STOP
        assert!(!EofContainer::is_prohibited_opcode(0x60)); // PUSH1
    }
}