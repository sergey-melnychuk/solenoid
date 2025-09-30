use eyre::Result;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use solenoid::common::hash;

fn main() -> Result<()> {
    // Signature data from the user
    let input_hex = "acee28ed6d5eff643274a2abd164fec12cc75f1ea78a87922304c04e2424bc88000000000000000000000000000000000000000000000000000000000000001c08da09260614b31b17af2ac76eaa7d50172b6d0cec03fe706748e2d532c0d3097e7a201aaefc664515b3a28a0bdd2fffdd58f3bff5fb639bf01f049c47648b3f";

    // Parse the components
    let input_bytes = hex::decode(input_hex)?;

    if input_bytes.len() != 128 {
        eyre::bail!("Invalid input length: expected 128 bytes, got {}", input_bytes.len());
    }

    let msg_hash = &input_bytes[0..32];
    let v_bytes = &input_bytes[32..64];
    let r_bytes = &input_bytes[64..96];
    let s_bytes = &input_bytes[96..128];

    println!("Message hash: 0x{}", hex::encode(msg_hash));
    println!("V parameter: 0x{}", hex::encode(v_bytes));
    println!("R parameter: 0x{}", hex::encode(r_bytes));
    println!("S parameter: 0x{}", hex::encode(s_bytes));

    // Convert v to recovery ID (v - 27)
    let v_byte = v_bytes[31]; // Last byte of the 32-byte word
    if v_byte != 27 && v_byte != 28 {
        eyre::bail!("Invalid v parameter: {}, expected 27 or 28", v_byte);
    }
    let recovery_id_byte = v_byte - 27;

    println!("Recovery ID: {}", recovery_id_byte);

    // Create signature from r and s
    let mut signature_bytes = [0u8; 64];
    signature_bytes[0..32].copy_from_slice(r_bytes);
    signature_bytes[32..64].copy_from_slice(s_bytes);

    let signature = Signature::from_slice(&signature_bytes)?;

    // Create recovery ID
    let recovery_id = RecoveryId::from_byte(recovery_id_byte)
        .ok_or_else(|| eyre::eyre!("Invalid recovery ID"))?;

    // Recover the public key
    let verifying_key = VerifyingKey::recover_from_prehash(msg_hash, &signature, recovery_id)?;

    // Convert public key to uncompressed format (65 bytes: 0x04 + 32 bytes x + 32 bytes y)
    let pubkey_encoded = verifying_key.to_encoded_point(false);
    let pubkey_bytes = pubkey_encoded.as_bytes();
    println!("Recovered public key: 0x{}", hex::encode(pubkey_bytes));

    // Hash the public key (without the 0x04 prefix) to get the Ethereum address
    let pubkey_hash = hash::keccak256(&pubkey_bytes[1..]);
    let address = &pubkey_hash[12..]; // Last 20 bytes

    println!("Recovered Ethereum address: 0x{}", hex::encode(address));

    Ok(())
}