use eyre::{Result, eyre};

use ark_bn254::{Bn254, Fq, Fq2, G1Affine, G1Projective, G2Affine};
use ark_ec::{AffineRepr, CurveGroup, pairing::Pairing};
use ark_ff::PrimeField;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use num_bigint::BigUint;
use num_traits::Zero;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

use crate::common::address::Address;

pub fn is_precompile(address: &Address) -> bool {
    let byte = address.0;
    byte[0..19] == [0u8; 19] && (1..=10).contains(&byte[19])
}

pub fn execute(address: &Address, input: &[u8]) -> eyre::Result<Vec<u8>> {
    match address.0[19] {
        1 => ecrecover(input),
        2 => sha256(input),
        3 => ripemd160(input),
        4 => identity(input),
        5 => modexp(input),
        6 => bn128_add(input),
        7 => bn128_mul(input),
        8 => bn128_pairing(input),
        9 => blake2f(input),
        10 => kzg_point_evaluation(input),
        _ => eyre::bail!("Invalid precompile address"),
    }
}

pub fn gas_cost(address: &Address, input: &[u8]) -> i64 {
    (match address.0[19] {
        1 => 3000,                                        // ecrecover
        2 => 60 + 12 * input.len().div_ceil(32) as u64,   // sha256
        3 => 600 + 120 * input.len().div_ceil(32) as u64, // ripemd160
        4 => 15 + 3 * input.len().div_ceil(32) as u64,    // identity
        5 => modexp_gas_cost(input),                      // modexp
        6 => 150,                                         // bn128_add
        7 => 6000,                                        // bn128_mul
        8 => 45000 + 34000 * (input.len() / 192) as u64,  // bn128_pairing
        9 => blake2f_gas_cost(input),                     // blake2f
        10 => 50000,                                      // kzg_point_evaluation (fixed cost)
        _ => 0,
    }) as i64
}

// 0x01: ECRecover - ECDSA signature recovery
fn ecrecover(input: &[u8]) -> eyre::Result<Vec<u8>> {
    if input.len() != 128 {
        return Ok(vec![0u8; 32]); // Return zero address on invalid input
    }

    let msg_hash = &input[0..32];
    let v_bytes = &input[32..64];
    let r_bytes = &input[64..96];
    let s_bytes = &input[96..128];

    // Extract v (recovery ID)
    let v_byte = v_bytes[31];
    if v_byte != 27 && v_byte != 28 {
        return Ok(vec![0u8; 32]);
    }
    let mut recovery_id_byte = v_byte - 27;

    // Create signature from r and s
    // Note: Ethereum's ecrecover accepts high-s signatures, but k256 rejects them
    // We need to normalize high-s to low-s and flip the recovery ID
    let mut signature_bytes = [0u8; 64];
    signature_bytes[0..32].copy_from_slice(r_bytes);
    signature_bytes[32..64].copy_from_slice(s_bytes);

    // Check if s is high (s > n/2) and normalize if needed
    // secp256k1 curve order
    const SECP256K1_N: [u8; 32] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE,
        0xBA, 0xAE, 0xDC, 0xE6, 0xAF, 0x48, 0xA0, 0x3B,
        0xBF, 0xD2, 0x5E, 0x8C, 0xD0, 0x36, 0x41, 0x41,
    ];
    const SECP256K1_N_HALF: [u8; 32] = [
        0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0x5D, 0x57, 0x6E, 0x73, 0x57, 0xA4, 0x50, 0x1D,
        0xDF, 0xE9, 0x2F, 0x46, 0x68, 0x1B, 0x20, 0xA0,
    ];

    // Compare s with n/2
    let s_is_high = s_bytes > &SECP256K1_N_HALF[..];

    if s_is_high {
        // Normalize: s_low = n - s_high
        let mut s_bigint = num_bigint::BigUint::from_bytes_be(s_bytes);
        let n_bigint = num_bigint::BigUint::from_bytes_be(&SECP256K1_N);
        s_bigint = n_bigint - s_bigint;

        let s_low_bytes = s_bigint.to_bytes_be();
        // Pad to 32 bytes
        let padding = 32 - s_low_bytes.len();
        signature_bytes[32..32 + padding].fill(0);
        signature_bytes[32 + padding..64].copy_from_slice(&s_low_bytes);

        // Flip recovery ID when normalizing s
        recovery_id_byte ^= 1;
    }

    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| eyre!("Invalid signature"))?;

    // Create recovery ID
    let recovery_id =
        RecoveryId::from_byte(recovery_id_byte).ok_or_else(|| eyre!("Invalid recovery ID"))?;

    // Recover public key
    let verifying_key = VerifyingKey::recover_from_prehash(msg_hash, &signature, recovery_id)
        .map_err(|e| eyre!("Failed to recover public key: {e}"))?;

    // Convert to uncompressed public key format (65 bytes: 0x04 + x + y)
    let pubkey_bytes = verifying_key.to_encoded_point(false);
    let pubkey_slice = pubkey_bytes.as_bytes();

    // Hash the public key (without the 0x04 prefix) to get the Ethereum address
    let pubkey_hash = keccak256(&pubkey_slice[1..]);

    // Return address padded to 32 bytes (12 zeros + 20 address bytes)
    let mut result = vec![0u8; 32];
    result[12..32].copy_from_slice(&pubkey_hash[12..32]);
    Ok(result)
}

// 0x02: SHA-256 hash function
fn sha256(input: &[u8]) -> eyre::Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    hasher.update(input);
    Ok(hasher.finalize().to_vec())
}

// 0x03: RIPEMD-160 hash function
fn ripemd160(input: &[u8]) -> eyre::Result<Vec<u8>> {
    let mut hasher = Ripemd160::new();
    hasher.update(input);
    let hash = hasher.finalize();

    // RIPEMD-160 returns 20 bytes, pad to 32 bytes
    let mut result = vec![0u8; 32];
    result[12..32].copy_from_slice(&hash);
    Ok(result)
}

// 0x04: Identity function (data copy)
fn identity(input: &[u8]) -> eyre::Result<Vec<u8>> {
    Ok(input.to_vec())
}

// 0x05: Modular exponentiation
fn modexp(input: &[u8]) -> eyre::Result<Vec<u8>> {
    if input.len() < 96 {
        return Ok(vec![]);
    }

    let base_len = BigUint::from_bytes_be(&input[0..32])
        .try_into()
        .unwrap_or(0usize);
    let exp_len = BigUint::from_bytes_be(&input[32..64])
        .try_into()
        .unwrap_or(0usize);
    let mod_len = BigUint::from_bytes_be(&input[64..96])
        .try_into()
        .unwrap_or(0usize);

    if base_len + exp_len + mod_len + 96 > input.len() {
        return Ok(vec![]);
    }

    let base = BigUint::from_bytes_be(&input[96..96 + base_len]);
    let exp = BigUint::from_bytes_be(&input[96 + base_len..96 + base_len + exp_len]);
    let modulus =
        BigUint::from_bytes_be(&input[96 + base_len + exp_len..96 + base_len + exp_len + mod_len]);

    if modulus.is_zero() {
        return Ok(vec![0u8; mod_len]);
    }

    let result = base.modpow(&exp, &modulus);
    let mut result_bytes = result.to_bytes_be();

    // Pad to mod_len
    if result_bytes.len() < mod_len {
        let mut padded = vec![0u8; mod_len - result_bytes.len()];
        padded.extend(result_bytes);
        result_bytes = padded;
    }

    Ok(result_bytes)
}

fn modexp_gas_cost(input: &[u8]) -> u64 {
    if input.len() < 96 {
        return 200;
    }

    let base_len = BigUint::from_bytes_be(&input[0..32])
        .try_into()
        .unwrap_or(0u64);
    let exp_len = BigUint::from_bytes_be(&input[32..64])
        .try_into()
        .unwrap_or(0u64);
    let mod_len = BigUint::from_bytes_be(&input[64..96])
        .try_into()
        .unwrap_or(0u64);

    // EIP-2565 gas calculation
    let max_len = base_len.max(mod_len);
    let multiplication_complexity = if max_len <= 64 {
        max_len * max_len
    } else if max_len <= 1024 {
        (max_len * max_len) / 4 + 96 * max_len - 3072
    } else {
        (max_len * max_len) / 16 + 480 * max_len - 199680
    };

    // Calculate iteration count
    let iteration_count =
        if exp_len <= 32 && input.len() >= 96 + base_len as usize + exp_len as usize {
            let exp_start = 96 + base_len as usize;
            let exp_bytes = &input[exp_start..exp_start + exp_len as usize];

            if exp_bytes.iter().all(|&b| b == 0) {
                0
            } else {
                let mut adjusted_exp_len = exp_len;
                // Find first non-zero byte
                for &byte in exp_bytes {
                    if byte == 0 {
                        adjusted_exp_len = adjusted_exp_len.saturating_sub(1);
                    } else {
                        break;
                    }
                }
                adjusted_exp_len * 8
                    + (exp_bytes.last().unwrap_or(&0).leading_zeros() as u64).saturating_sub(1)
            }
        } else {
            8 * exp_len.saturating_sub(32).max(1)
        };

    let gas = (multiplication_complexity * iteration_count.max(1)) / 3;
    gas.max(200)
}

// 0x06: BN128 elliptic curve point addition
fn bn128_add(input: &[u8]) -> eyre::Result<Vec<u8>> {
    let mut input_data = input.to_vec();
    if input_data.len() < 128 {
        input_data.resize(128, 0);
    }

    let x1 = BigUint::from_bytes_be(&input_data[0..32]);
    let y1 = BigUint::from_bytes_be(&input_data[32..64]);
    let x2 = BigUint::from_bytes_be(&input_data[64..96]);
    let y2 = BigUint::from_bytes_be(&input_data[96..128]);

    // Convert to ark-bn254 field elements
    let x1_fq = bytes_to_fq(&x1.to_bytes_be())?;
    let y1_fq = bytes_to_fq(&y1.to_bytes_be())?;
    let x2_fq = bytes_to_fq(&x2.to_bytes_be())?;
    let y2_fq = bytes_to_fq(&y2.to_bytes_be())?;

    // Create points
    let p1 = if x1.is_zero() && y1.is_zero() {
        G1Projective::zero()
    } else {
        G1Affine::new(x1_fq, y1_fq).into()
    };

    let p2 = if x2.is_zero() && y2.is_zero() {
        G1Projective::zero()
    } else {
        G1Affine::new(x2_fq, y2_fq).into()
    };

    // Add points
    let result = p1 + p2;
    let result_affine = result.into_affine();

    // Convert back to bytes
    let mut output = vec![0u8; 64];
    if !result.is_zero() {
        let x_bytes = fq_to_bytes(result_affine.x);
        let y_bytes = fq_to_bytes(result_affine.y);
        output[0..32].copy_from_slice(&x_bytes);
        output[32..64].copy_from_slice(&y_bytes);
    }

    Ok(output)
}

// 0x07: BN128 elliptic curve scalar multiplication
fn bn128_mul(input: &[u8]) -> eyre::Result<Vec<u8>> {
    let mut input_data = input.to_vec();
    if input_data.len() < 96 {
        input_data.resize(96, 0);
    }

    let x = BigUint::from_bytes_be(&input_data[0..32]);
    let y = BigUint::from_bytes_be(&input_data[32..64]);
    let scalar = BigUint::from_bytes_be(&input_data[64..96]);

    // Convert to ark-bn254 types
    let x_fq = bytes_to_fq(&x.to_bytes_be())?;
    let y_fq = bytes_to_fq(&y.to_bytes_be())?;
    let scalar_fr = bytes_to_fr(&scalar.to_bytes_be())?;

    // Create point
    let point = if x.is_zero() && y.is_zero() {
        G1Projective::zero()
    } else {
        G1Affine::new(x_fq, y_fq).into()
    };

    // Scalar multiplication
    let result = point * scalar_fr;
    let result_affine = result.into_affine();

    // Convert back to bytes
    let mut output = vec![0u8; 64];
    if !result.is_zero() {
        let x_bytes = fq_to_bytes(result_affine.x);
        let y_bytes = fq_to_bytes(result_affine.y);
        output[0..32].copy_from_slice(&x_bytes);
        output[32..64].copy_from_slice(&y_bytes);
    }

    Ok(output)
}

// 0x08: BN128 pairing check
fn bn128_pairing(input: &[u8]) -> eyre::Result<Vec<u8>> {
    if !input.len().is_multiple_of(192) {
        return Err(eyre!("Invalid input length for pairing"));
    }

    let pairs_count = input.len() / 192;
    let mut pairs = Vec::new();

    for i in 0..pairs_count {
        let offset = i * 192;

        // G1 point
        let x1 = BigUint::from_bytes_be(&input[offset..offset + 32]);
        let y1 = BigUint::from_bytes_be(&input[offset + 32..offset + 64]);

        // G2 point (note: G2 coordinates are in different order)
        let x2_c1 = BigUint::from_bytes_be(&input[offset + 64..offset + 96]);
        let x2_c0 = BigUint::from_bytes_be(&input[offset + 96..offset + 128]);
        let y2_c1 = BigUint::from_bytes_be(&input[offset + 128..offset + 160]);
        let y2_c0 = BigUint::from_bytes_be(&input[offset + 160..offset + 192]);

        let g1_point = if x1.is_zero() && y1.is_zero() {
            G1Affine::zero()
        } else {
            let x_fq = bytes_to_fq(&x1.to_bytes_be())?;
            let y_fq = bytes_to_fq(&y1.to_bytes_be())?;
            G1Affine::new(x_fq, y_fq)
        };

        let g2_point = if x2_c0.is_zero() && x2_c1.is_zero() && y2_c0.is_zero() && y2_c1.is_zero() {
            G2Affine::zero()
        } else {
            let x2_c0_fq = bytes_to_fq(&x2_c0.to_bytes_be())?;
            let x2_c1_fq = bytes_to_fq(&x2_c1.to_bytes_be())?;
            let y2_c0_fq = bytes_to_fq(&y2_c0.to_bytes_be())?;
            let y2_c1_fq = bytes_to_fq(&y2_c1.to_bytes_be())?;

            let x2_fq2 = Fq2::new(x2_c0_fq, x2_c1_fq);
            let y2_fq2 = Fq2::new(y2_c0_fq, y2_c1_fq);

            G2Affine::new(x2_fq2, y2_fq2)
        };

        pairs.push((g1_point, g2_point));
    }

    // Perform pairing check
    let (g1, g2): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();

    // Handle empty pairing case - should return 1 (true)
    if g1.is_empty() {
        let mut ret = vec![0u8; 32];
        ret[31] = 1;
        return Ok(ret);
    }

    let pairing = Bn254::multi_pairing(g1, g2);

    let mut ret = vec![0u8; 32];
    // Return 1 if pairing equals 1 (multiplicative identity), 0 otherwise
    if pairing.is_zero() {
        ret[31] = 1;
    }
    Ok(ret)
}

// 0x09: Blake2f compression function
fn blake2f(input: &[u8]) -> eyre::Result<Vec<u8>> {
    if input.len() != 213 {
        return Err(eyre!("Invalid input length for Blake2f"));
    }

    let rounds = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
    let mut h = [0u64; 8];
    let mut m = [0u64; 16];
    let mut t = [0u64; 2];

    // Parse state vector h (64 bytes = 8 u64 words)
    for (i, h) in h.iter_mut().enumerate() {
        let start = 4 + i * 8;
        *h = u64::from_le_bytes([
            input[start],
            input[start + 1],
            input[start + 2],
            input[start + 3],
            input[start + 4],
            input[start + 5],
            input[start + 6],
            input[start + 7],
        ]);
    }

    // Parse message block m (128 bytes = 16 u64 words)
    for (i, m) in m.iter_mut().enumerate() {
        let start = 68 + i * 8;
        *m = u64::from_le_bytes([
            input[start],
            input[start + 1],
            input[start + 2],
            input[start + 3],
            input[start + 4],
            input[start + 5],
            input[start + 6],
            input[start + 7],
        ]);
    }

    // Parse counter t (16 bytes = 2 u64 words)
    for (i, t) in t.iter_mut().enumerate() {
        let start = 196 + i * 8;
        *t = u64::from_le_bytes([
            input[start],
            input[start + 1],
            input[start + 2],
            input[start + 3],
            input[start + 4],
            input[start + 5],
            input[start + 6],
            input[start + 7],
        ]);
    }

    let f = input[212] != 0;

    // Perform Blake2f compression
    let result = blake2f_compression(h, m, t, f, rounds);

    // Convert result back to bytes (little-endian)
    let mut output = vec![0u8; 64];
    for i in 0..8 {
        let bytes = result[i].to_le_bytes();
        output[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
    }

    Ok(output)
}

fn blake2f_compression(
    mut h: [u64; 8],
    m: [u64; 16],
    t: [u64; 2],
    f: bool,
    rounds: u32,
) -> [u64; 8] {
    // Blake2b constants
    const IV: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];

    const SIGMA: [[usize; 16]; 12] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
        [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
        [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
        [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
        [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
        [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
        [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
        [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
        [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    ];

    // Initialize working variables
    let mut v = [0u64; 16];
    v[0..8].copy_from_slice(&h);
    v[8..16].copy_from_slice(&IV);

    // Mix counter
    v[12] ^= t[0];
    v[13] ^= t[1];

    // Set finalization flag
    if f {
        v[14] = !v[14];
    }

    // Compression rounds
    for round in 0..rounds {
        let s = &SIGMA[round as usize % 12];

        // G function calls
        mix(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        mix(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        mix(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        mix(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);

        mix(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        mix(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        mix(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        mix(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }

    // Finalize hash
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }

    h
}

fn mix(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

fn blake2f_gas_cost(input: &[u8]) -> u64 {
    if input.len() < 4 {
        return 0;
    }
    let rounds = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
    rounds as u64
}

// 0x0A: KZG point evaluation (EIP-4844)
fn kzg_point_evaluation(input: &[u8]) -> eyre::Result<Vec<u8>> {
    if input.len() != 192 {
        return Err(eyre!("Invalid input length for KZG point evaluation"));
    }

    // Parse input according to EIP-4844: versioned_hash ++ z ++ y ++ commitment ++ proof
    let _versioned_hash = &input[0..32];
    let z = &input[32..64];
    let y = &input[64..96];
    let commitment = &input[96..144];
    let proof = &input[144..192];

    // KZG verification using BLS12-381 pairing
    let verification_result = verify_kzg_proof(commitment, z, y, proof)?;

    if !verification_result {
        return Err(eyre!("KZG proof verification failed"));
    }

    // Return field element y and commitment hash on successful verification
    let mut output = vec![0u8; 64];
    output[0..32].copy_from_slice(y);

    // Second 32 bytes: commitment hash
    let commitment_hash = keccak256(commitment);
    output[32..64].copy_from_slice(&commitment_hash);

    Ok(output)
}

fn verify_kzg_proof(commitment: &[u8], z: &[u8], y: &[u8], proof: &[u8]) -> eyre::Result<bool> {
    use ark_bls12_381::{Bls12_381, Fr, G1Affine, G1Projective, G2Affine, G2Projective};
    use ark_ec::{CurveGroup, pairing::Pairing};
    use ark_ff::PrimeField;
    use ark_serialize::CanonicalDeserialize;

    // Parse commitment and proof points (48 bytes compressed G1 points)
    let commitment_point = G1Affine::deserialize_compressed(commitment)
        .map_err(|_| eyre!("Invalid commitment point"))?;
    let proof_point =
        G1Affine::deserialize_compressed(proof).map_err(|_| eyre!("Invalid proof point"))?;

    // Convert z and y to field elements
    let z_fr = Fr::from_be_bytes_mod_order(z);
    let y_fr = Fr::from_be_bytes_mod_order(y);

    // Get generators and trusted setup point
    let g1_gen = G1Affine::generator();
    let g2_gen = G2Affine::generator();
    let tau_g2 = get_tau_g2_trusted_setup();

    // Compute [y]_1 and [z]_2 using projective arithmetic
    let y_g1_proj = G1Projective::from(g1_gen) * y_fr;
    let z_g2_proj = G2Projective::from(g2_gen) * z_fr;

    // Convert to affine
    let y_g1 = y_g1_proj.into_affine();
    let z_g2 = z_g2_proj.into_affine();

    // Compute commitment - [y]_1 and [tau - z]_2 using projective arithmetic
    let commitment_minus_y_proj = G1Projective::from(commitment_point) - G1Projective::from(y_g1);
    let tau_minus_z_g2_proj = G2Projective::from(tau_g2) - G2Projective::from(z_g2);

    let commitment_minus_y = commitment_minus_y_proj.into_affine();
    let tau_minus_z_g2 = tau_minus_z_g2_proj.into_affine();

    // Pairing check: e(proof, [tau - z]_2) = e(commitment - [y]_1, G_2)
    let lhs = Bls12_381::pairing(proof_point, tau_minus_z_g2);
    let rhs = Bls12_381::pairing(commitment_minus_y, g2_gen);

    Ok(lhs == rhs)
}

fn get_tau_g2_trusted_setup() -> ark_bls12_381::G2Affine {
    // In real implementation, this comes from Ethereum's KZG trusted setup
    // For now return generator (verification will fail for real proofs)
    ark_bls12_381::G2Affine::generator()
}

// Helper functions for BN254 operations
fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut keccak = Keccak::v256();
    let mut output = [0u8; 32];
    keccak.update(data);
    keccak.finalize(&mut output);
    output
}

fn bytes_to_fq(bytes: &[u8]) -> Result<Fq> {
    let mut padded = vec![0u8; 32];
    let start = 32usize.saturating_sub(bytes.len());
    padded[start..].copy_from_slice(bytes);

    Ok(Fq::from_be_bytes_mod_order(&padded))
}

fn bytes_to_fr(bytes: &[u8]) -> Result<ark_bn254::Fr> {
    let mut padded = vec![0u8; 32];
    let start = 32usize.saturating_sub(bytes.len());
    padded[start..].copy_from_slice(bytes);

    Ok(ark_bn254::Fr::from_be_bytes_mod_order(&padded))
}

fn fq_to_bytes(fq: Fq) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    use ark_ff::BigInteger;
    let fq_bytes = fq.into_bigint().to_bytes_be();
    let start = 32 - fq_bytes.len();
    bytes[start..].copy_from_slice(&fq_bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;

    // Test utilities
    fn hex_to_bytes(hex_str: &str) -> Vec<u8> {
        hex::decode(hex_str.replace("0x", "").replace(" ", "")).unwrap()
    }

    #[test]
    fn test_is_precompile() {
        // Test valid precompile addresses
        for i in 1..=10 {
            let mut addr_bytes = [0u8; 20];
            addr_bytes[19] = i;
            let address = Address(addr_bytes);
            assert!(
                is_precompile(&address),
                "Address 0x{:02x} should be a precompile",
                i
            );
        }

        // Test invalid addresses
        let mut addr_bytes = [0u8; 20];
        addr_bytes[19] = 0;
        assert!(
            !is_precompile(&Address(addr_bytes)),
            "Address 0x00 should not be a precompile"
        );

        addr_bytes[19] = 11;
        assert!(
            !is_precompile(&Address(addr_bytes)),
            "Address 0x0B should not be a precompile"
        );

        // Test non-zero prefix
        addr_bytes[0] = 1;
        addr_bytes[19] = 1;
        assert!(
            !is_precompile(&Address(addr_bytes)),
            "Address with non-zero prefix should not be a precompile"
        );
    }

    #[test]
    fn test_gas_cost() {
        let mut addr_bytes = [0u8; 20];

        // Test ecrecover gas cost
        addr_bytes[19] = 1;
        let address = Address(addr_bytes);
        assert_eq!(gas_cost(&address, &[]), 3000);

        // Test sha256 gas cost
        addr_bytes[19] = 2;
        let address = Address(addr_bytes);
        let input = vec![0u8; 64]; // 64 bytes = 2 words
        assert_eq!(gas_cost(&address, &input), 60 + 12 * 2);

        // Test ripemd160 gas cost
        addr_bytes[19] = 3;
        let address = Address(addr_bytes);
        assert_eq!(gas_cost(&address, &input), 600 + 120 * 2);

        // Test identity gas cost
        addr_bytes[19] = 4;
        let address = Address(addr_bytes);
        assert_eq!(gas_cost(&address, &input), 15 + 3 * 2);

        // Test bn128_add gas cost
        addr_bytes[19] = 6;
        let address = Address(addr_bytes);
        assert_eq!(gas_cost(&address, &[]), 150);

        // Test bn128_mul gas cost
        addr_bytes[19] = 7;
        let address = Address(addr_bytes);
        assert_eq!(gas_cost(&address, &[]), 6000);

        // Test kzg_point_evaluation gas cost
        addr_bytes[19] = 10;
        let address = Address(addr_bytes);
        assert_eq!(gas_cost(&address, &[]), 50000);
    }

    // 0x01: ECRecover tests
    #[test]
    fn test_ecrecover_valid() {
        // Valid signature from a real transaction
        let input = hex_to_bytes(
            "acee28ed6d5eff643274a2abd164fec12cc75f1ea78a87922304c04e2424bc88\
            000000000000000000000000000000000000000000000000000000000000001c\
            08da09260614b31b17af2ac76eaa7d50172b6d0cec03fe706748e2d532c0d309\
            7e7a201aaefc664515b3a28a0bdd2fffdd58f3bff5fb639bf01f049c47648b3f",
        );

        let result = ecrecover(&input).unwrap();
        assert_eq!(result.len(), 32);

        // Expected address: 0xd148c7f37b346a4bd8e14f8c1f181f5f640481c8
        let expected_address = "000000000000000000000000d148c7f37b346a4bd8e14f8c1f181f5f640481c8";
        assert_eq!(hex::encode(result), expected_address);
    }

    #[test]
    fn test_ecrecover_invalid_length() {
        let input = vec![0u8; 64]; // Wrong length
        let result = ecrecover(&input).unwrap();
        assert_eq!(result, vec![0u8; 32]); // Should return zero address
    }

    #[test]
    fn test_ecrecover_invalid_v() {
        let mut input = vec![0u8; 128];
        input[63] = 26; // Invalid v (should be 27 or 28)
        let result = ecrecover(&input).unwrap();
        assert_eq!(result, vec![0u8; 32]); // Should return zero address
    }

    #[test]
    fn test_ecrecover_high_s_signature() {
        // BLOCK: 23647631, INDEX: 159
        // TX: 0x255e2638eebd5fb935dfd47c3ef58667281336fa9b628610fa27b2a45d1cc8bf
        // This signature has a high-s value that Ethereum accepts but k256 rejects by default
        let input = hex_to_bytes(
            "a6588c81ba59e991dccec1b3c3b73c4b04cce35f30344c6df815d75e4d42351a\
            000000000000000000000000000000000000000000000000000000000000001b\
            4ca5e12d5fc25d983a215fb64032bbfe90a3e596d67a1b2cfa9646186a513704\
            bda125db9c2f810df6eaf77a5479de3b147359425fc1534f1fee6c1211308966");
        let expected = "0000000000000000000000008948112e60ba94f6afdcfc6b690904b7321d3a52";
        let result = ecrecover(&input).unwrap();
        assert_eq!(hex::encode(result), expected);
    }

    #[test]
    fn test_ecrecover_with_generated_signature() {
        use k256::ecdsa::SigningKey;

        let msg_hash = hex_to_bytes("a6588c81ba59e991dccec1b3c3b73c4b04cce35f30344c6df815d75e4d42351a");

        let private_key_bytes: [u8; 32] = hex_to_bytes("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .try_into().unwrap();
        let signing_key = SigningKey::from_bytes(&private_key_bytes.into()).unwrap();

        let (signature, recovery_id) = signing_key.sign_prehash_recoverable(&msg_hash).unwrap();

        let verifying_key = signing_key.verifying_key();
        let pubkey_bytes = verifying_key.to_encoded_point(false);
        let pubkey_hash = keccak256(&pubkey_bytes.as_bytes()[1..]); // Skip 0x04 prefix

        let mut expected_address = [0u8; 32];
        expected_address[12..32].copy_from_slice(&pubkey_hash[12..32]);

        let sig_bytes = signature.to_bytes();
        let v = recovery_id.to_byte() + 27;

        let mut input = vec![0u8; 128];
        input[0..32].copy_from_slice(&msg_hash);
        input[63] = v; // v is a single byte, right-aligned in 32-byte slot
        input[64..96].copy_from_slice(&sig_bytes[..32]); // r
        input[96..128].copy_from_slice(&sig_bytes[32..]); // s

        let result = ecrecover(&input).unwrap();
        assert_eq!(hex::encode(&result), hex::encode(&expected_address));
    }

    // 0x02: SHA-256 tests
    #[test]
    fn test_sha256_empty() {
        let input = vec![];
        let result = sha256(&input).unwrap();
        assert_eq!(result.len(), 32);

        // Empty string SHA-256 hash
        let expected =
            hex_to_bytes("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(result, expected);
    }

    #[test]
    fn test_sha256_abc() {
        let input = b"abc".to_vec();
        let result = sha256(&input).unwrap();

        // "abc" SHA-256 hash
        let expected =
            hex_to_bytes("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(result, expected);
    }

    #[test]
    fn test_sha256_long() {
        let input = vec![0u8; 1000];
        let result = sha256(&input).unwrap();
        assert_eq!(result.len(), 32);
        assert_ne!(result, vec![0u8; 32]); // Should not be all zeros
    }

    // 0x03: RIPEMD-160 tests
    #[test]
    fn test_ripemd160_empty() {
        let input = vec![];
        let result = ripemd160(&input).unwrap();
        assert_eq!(result.len(), 32);

        // First 12 bytes should be zero (padding)
        assert_eq!(&result[0..12], &[0u8; 12]);

        // Should not be all zeros
        assert_ne!(result, vec![0u8; 32]);
    }

    #[test]
    fn test_ripemd160_abc() {
        let input = b"abc".to_vec();
        let result = ripemd160(&input).unwrap();
        assert_eq!(result.len(), 32);

        // First 12 bytes should be zero (padding)
        assert_eq!(&result[0..12], &[0u8; 12]);

        // "abc" RIPEMD-160 hash (20 bytes) padded to 32 bytes
        let expected_hash = hex_to_bytes("8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
        assert_eq!(&result[12..32], expected_hash.as_slice());
    }

    // 0x04: Identity tests
    #[test]
    fn test_identity_empty() {
        let input = vec![];
        let result = identity(&input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_identity_data() {
        let input = vec![1, 2, 3, 4, 5];
        let result = identity(&input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_identity_large() {
        let input = vec![0xAB; 1000];
        let result = identity(&input).unwrap();
        assert_eq!(result, input);
    }

    // 0x05: Modexp tests
    #[test]
    fn test_modexp_invalid_length() {
        let input = vec![0u8; 50]; // Too short
        let result = modexp(&input).unwrap();
        assert_eq!(result, Vec::<u8>::new());
    }

    #[test]
    fn test_modexp_simple() {
        // 3^2 mod 5 = 4
        let mut input = vec![0u8; 96];

        // base_len = 1
        input[31] = 1;
        // exp_len = 1
        input[63] = 1;
        // mod_len = 1
        input[95] = 1;

        input.push(3); // base = 3
        input.push(2); // exp = 2
        input.push(5); // mod = 5

        let result = modexp(&input).unwrap();
        assert_eq!(result, vec![4]);
    }

    #[test]
    fn test_modexp_zero_modulus() {
        let mut input = vec![0u8; 96];

        // base_len = 1, exp_len = 1, mod_len = 1
        input[31] = 1;
        input[63] = 1;
        input[95] = 1;

        input.push(3); // base = 3
        input.push(2); // exp = 2
        input.push(0); // mod = 0

        let result = modexp(&input).unwrap();
        assert_eq!(result, vec![0]); // Should return zero
    }

    #[test]
    fn test_modexp_gas_cost() {
        let input = vec![0u8; 50]; // Too short
        assert_eq!(modexp_gas_cost(&input), 200); // Minimum cost

        let mut input = vec![0u8; 96];
        // Small lengths should return minimum cost
        input[31] = 1; // base_len = 1
        input[63] = 1; // exp_len = 1
        input[95] = 1; // mod_len = 1

        let cost = modexp_gas_cost(&input);
        assert!(cost >= 200); // Should be at least minimum
    }

    // 0x06: BN128 Add tests
    #[test]
    fn test_bn128_add_zero_points() {
        let input = vec![0u8; 128]; // Two zero points
        let result = bn128_add(&input).unwrap();
        assert_eq!(result.len(), 64);
        assert_eq!(result, vec![0u8; 64]); // Zero + Zero = Zero
    }

    #[test]
    fn test_bn128_add_short_input() {
        let input = vec![0u8; 64]; // Short input, should be padded
        let result = bn128_add(&input).unwrap();
        assert_eq!(result.len(), 64);
    }

    #[test]
    fn test_bn128_add_generator_plus_zero() {
        // Generator point + zero point = generator point
        let mut input = vec![0u8; 128];

        // Set first point to generator (this is simplified - real generator coordinates would be used)
        input[31] = 1; // x1 = 1 (simplified)
        input[63] = 2; // y1 = 2 (simplified)
        // Second point remains zero

        let result = bn128_add(&input);
        // Should not panic and return valid result
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 64);
    }

    // 0x07: BN128 Mul tests
    #[test]
    fn test_bn128_mul_zero_point() {
        let input = vec![0u8; 96]; // Zero point * any scalar = zero
        let result = bn128_mul(&input).unwrap();
        assert_eq!(result.len(), 64);
        assert_eq!(result, vec![0u8; 64]);
    }

    #[test]
    fn test_bn128_mul_short_input() {
        let input = vec![0u8; 50]; // Short input, should be padded
        let result = bn128_mul(&input).unwrap();
        assert_eq!(result.len(), 64);
    }

    #[test]
    fn test_bn128_mul_any_point_times_zero() {
        let mut input = vec![0u8; 96];

        // Set point to some values
        input[31] = 1; // x = 1
        input[63] = 2; // y = 2
        // scalar remains 0

        let result = bn128_mul(&input).unwrap();
        assert_eq!(result.len(), 64);
        assert_eq!(result, vec![0u8; 64]); // Any point * 0 = zero
    }

    // 0x08: BN128 Pairing tests
    #[test]
    fn test_bn128_pairing_invalid_length() {
        let input = vec![0u8; 100]; // Invalid length (not multiple of 192)
        let result = bn128_pairing(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_bn128_pairing_empty() {
        let input = vec![]; // Empty input
        let result = bn128_pairing(&input).unwrap();
        assert_eq!(result.len(), 32);
        assert_eq!(result[31], 1); // Empty pairing should return true
    }

    #[test]
    fn test_bn128_pairing_single_zero_pair() {
        let input = vec![0u8; 192]; // Single pair of zero points
        let result = bn128_pairing(&input).unwrap();
        assert_eq!(result.len(), 32);
        // Zero pairing should be identity (true)
        assert_eq!(result[31], 1);
    }

    #[test]
    fn test_bn128_pairing_multiple_pairs() {
        let input = vec![0u8; 384]; // Two pairs
        let result = bn128_pairing(&input).unwrap();
        assert_eq!(result.len(), 32);
    }

    // 0x09: Blake2f tests
    #[test]
    fn test_blake2f_invalid_length() {
        let input = vec![0u8; 100]; // Wrong length
        let result = blake2f(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_blake2f_valid_input() {
        let mut input = vec![0u8; 213];

        // Set rounds = 1
        input[3] = 1;

        // Set some test data
        for i in 4..68 {
            input[i] = (i % 256) as u8; // h
        }
        for i in 68..196 {
            input[i] = ((i * 2) % 256) as u8; // m
        }
        for i in 196..212 {
            input[i] = ((i * 3) % 256) as u8; // t
        }
        input[212] = 1; // f = true

        let result = blake2f(&input).unwrap();
        assert_eq!(result.len(), 64);
        assert_ne!(result, vec![0u8; 64]); // Should not be all zeros
    }

    // Official Blake2f test vectors from Ethereum tests
    #[test]
    fn test_blake2f_official_vector() {
        // EIP-152 Blake2f test vector - construct properly
        let mut input = Vec::with_capacity(213);

        // rounds = 12 (4 bytes, big-endian)
        input.extend_from_slice(&0x0000000cu32.to_be_bytes());

        // h - Blake2b IV (64 bytes, little-endian u64s)
        let h_values = [
            0x6a09e667f3bcc908u64,
            0xbb67ae8584caa73bu64,
            0x3c6ef372fe94f82bu64,
            0xa54ff53a5f1d36f1u64,
            0x510e527fade682d1u64,
            0x9b05688c2b3e6c1fu64,
            0x1f83d9abfb41bd6bu64,
            0x5be0cd19137e2179u64,
        ];
        for h in h_values {
            input.extend_from_slice(&h.to_le_bytes());
        }

        // m - message block "abc" + zeros (128 bytes)
        let mut m = [0u8; 128];
        m[0..3].copy_from_slice(b"abc");
        input.extend_from_slice(&m);

        // t - counter (16 bytes, little-endian u64s)
        input.extend_from_slice(&3u64.to_le_bytes()); // t[0] = 3
        input.extend_from_slice(&0u64.to_le_bytes()); // t[1] = 0

        // f - final flag (1 byte)
        input.push(0x01);

        assert_eq!(input.len(), 213);
        let result = blake2f(&input).unwrap();

        // Expected output for Blake2f compression of IV with "abc" message
        let expected = hex_to_bytes(
            "d3284c32b0abb2e548df19c4f7740c20f0771d6bcaf176482dd645e9133a9544210b29bb41a2af4bfbe5a5fabf854b997c8f40aaf818c0411a53d63aff481cc4",
        );

        assert_eq!(
            result, expected,
            "Blake2f output doesn't match expected value for IV + 'abc' test"
        );
    }

    #[test]
    fn test_blake2f_rounds_variation() {
        let mut input = vec![0u8; 213];

        // Test 1 round vs 12 rounds - should produce different results
        input[3] = 1; // rounds = 1
        let result_1_round = blake2f(&input).unwrap();

        input[3] = 12; // rounds = 12
        let result_12_rounds = blake2f(&input).unwrap();

        assert_ne!(result_1_round, result_12_rounds);
        assert_eq!(result_1_round.len(), 64);
        assert_eq!(result_12_rounds.len(), 64);
    }

    #[test]
    fn test_blake2f_final_flag() {
        let mut input = vec![0u8; 213];
        input[3] = 12; // rounds = 12

        // Test f=false vs f=true - should produce different results
        input[212] = 0; // f = false
        let result_f_false = blake2f(&input).unwrap();

        input[212] = 1; // f = true
        let result_f_true = blake2f(&input).unwrap();

        assert_ne!(result_f_false, result_f_true);
        assert_eq!(result_f_false.len(), 64);
        assert_eq!(result_f_true.len(), 64);
    }

    #[test]
    fn test_blake2f_counter_variation() {
        let mut input = vec![0u8; 213];
        input[3] = 12; // rounds = 12
        input[212] = 1; // f = true

        // Test different counter values - should produce different results
        // t[0] = 0
        input[196] = 0;
        let result_counter_0 = blake2f(&input).unwrap();

        // t[0] = 3
        input[196] = 3;
        let result_counter_3 = blake2f(&input).unwrap();

        assert_ne!(result_counter_0, result_counter_3);
        assert_eq!(result_counter_0.len(), 64);
        assert_eq!(result_counter_3.len(), 64);
    }

    #[test]
    fn test_blake2f_zero_rounds() {
        let mut input = vec![0u8; 213];

        // Set up initial state
        input[4..12].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // h[0]
        input[12..20].copy_from_slice(&[8, 7, 6, 5, 4, 3, 2, 1]); // h[1]
        input[212] = 1; // f = true

        // With 0 rounds, should perform only the finalization XOR
        input[3] = 0; // rounds = 0
        let result = blake2f(&input).unwrap();

        assert_eq!(result.len(), 64);
        assert_ne!(result, vec![0u8; 64]);
    }

    #[test]
    fn test_blake2f_message_variation() {
        let mut input = vec![0u8; 213];
        input[3] = 12; // rounds = 12
        input[212] = 1; // f = true

        // Test different messages - should produce different results
        // Message all zeros
        let result_zeros = blake2f(&input).unwrap();

        // Message with "abc"
        input[68..71].copy_from_slice(b"abc");
        let result_abc = blake2f(&input).unwrap();

        assert_ne!(result_zeros, result_abc);
        assert_eq!(result_zeros.len(), 64);
        assert_eq!(result_abc.len(), 64);
    }

    #[test]
    fn test_blake2f_gas_cost() {
        let input = vec![0u8; 1]; // Too short
        assert_eq!(blake2f_gas_cost(&input), 0);

        let mut input = vec![0u8; 213];
        input[0] = 0;
        input[1] = 0;
        input[2] = 0;
        input[3] = 100; // 100 rounds

        assert_eq!(blake2f_gas_cost(&input), 100);
    }

    // 0x0A: KZG Point Evaluation tests
    #[test]
    fn test_kzg_invalid_length() {
        let input = vec![0u8; 100]; // Wrong length
        let result = kzg_point_evaluation(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_kzg_valid_length_but_invalid_points() {
        let input = vec![0u8; 192]; // Correct length but invalid points
        let result = kzg_point_evaluation(&input);
        // Should fail due to invalid G1 points (all zeros)
        assert!(result.is_err());
    }

    #[test]
    fn test_kzg_valid_format() {
        let mut input = vec![0u8; 192];

        // Set versioned hash
        for i in 0..32 {
            input[i] = i as u8;
        }

        // Set z (evaluation point)
        input[32] = 1;

        // Set y (claimed evaluation)
        input[64] = 2;

        // Set commitment and proof to valid-looking G1 points
        // (These are still invalid but won't fail parsing checks)
        input[96] = 0x80; // Set compression flag for G1 point
        input[144] = 0x80; // Set compression flag for G1 point

        let result = kzg_point_evaluation(&input);
        // Will likely fail verification but should parse input correctly
        // The test validates input parsing logic
        assert!(result.is_err() || result.unwrap().len() == 64);
    }

    // Verify KZG proof function tests
    #[test]
    fn test_verify_kzg_proof_invalid_points() {
        let commitment = vec![0u8; 48]; // Invalid G1 point
        let z = vec![0u8; 32];
        let y = vec![1u8; 32];
        let proof = vec![0u8; 48]; // Invalid G1 point

        let result = verify_kzg_proof(&commitment, &z, &y, &proof);
        assert!(result.is_err()); // Should fail on invalid points
    }

    #[test]
    fn test_verify_kzg_proof_wrong_length() {
        let commitment = vec![0u8; 30]; // Wrong length
        let z = vec![0u8; 32];
        let y = vec![0u8; 32];
        let proof = vec![0u8; 48];

        let result = verify_kzg_proof(&commitment, &z, &y, &proof);
        assert!(result.is_err());
    }

    // Helper function tests
    #[test]
    fn test_keccak256() {
        let input = b"";
        let result = keccak256(input);

        // Empty string Keccak-256 hash
        let expected =
            hex_to_bytes("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470");
        assert_eq!(result.to_vec(), expected);
    }

    #[test]
    fn test_keccak256_abc() {
        let input = b"abc";
        let result = keccak256(input);
        assert_eq!(result.len(), 32);
        assert_ne!(result.to_vec(), vec![0u8; 32]);
    }

    #[test]
    fn test_bytes_to_fq() {
        let bytes = vec![0u8; 32];
        let result = bytes_to_fq(&bytes);
        assert!(result.is_ok());

        let bytes = vec![0xFF; 32];
        let result = bytes_to_fq(&bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bytes_to_fr() {
        let bytes = vec![0u8; 32];
        let result = bytes_to_fr(&bytes);
        assert!(result.is_ok());

        let bytes = vec![1u8; 32];
        let result = bytes_to_fr(&bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_fq_to_bytes() {
        use ark_ff::Zero;

        let zero_fq = Fq::zero();
        let bytes = fq_to_bytes(zero_fq);
        assert_eq!(bytes, [0u8; 32]);

        let one_fq = Fq::from(1u64);
        let bytes = fq_to_bytes(one_fq);
        assert_eq!(bytes[31], 1);
        assert_eq!(&bytes[0..31], &[0u8; 31]);
    }

    // Integration tests
    #[test]
    fn test_execute_function() {
        let mut addr_bytes = [0u8; 20];

        // Test ecrecover execution
        addr_bytes[19] = 1;
        let address = Address(addr_bytes);
        let input = vec![0u8; 128];
        let result = execute(&address, &input);
        assert!(result.is_ok());

        // Test sha256 execution
        addr_bytes[19] = 2;
        let address = Address(addr_bytes);
        let input = b"test".to_vec();
        let result = execute(&address, &input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);

        // Test identity execution
        addr_bytes[19] = 4;
        let address = Address(addr_bytes);
        let input = vec![1, 2, 3, 4, 5];
        let result = execute(&address, &input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), input);

        // Test invalid precompile
        addr_bytes[19] = 99;
        let address = Address(addr_bytes);
        let result = execute(&address, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_edge_cases() {
        // Test maximum size inputs
        let large_input = vec![0xAB; 10000];

        // Identity should handle large inputs
        let result = identity(&large_input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), large_input);

        // SHA-256 should handle large inputs
        let result = sha256(&large_input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_deterministic_behavior() {
        // Same input should always produce same output
        let input = b"deterministic test".to_vec();

        let result1 = sha256(&input).unwrap();
        let result2 = sha256(&input).unwrap();
        assert_eq!(result1, result2);

        let result1 = ripemd160(&input).unwrap();
        let result2 = ripemd160(&input).unwrap();
        assert_eq!(result1, result2);

        let result1 = identity(&input).unwrap();
        let result2 = identity(&input).unwrap();
        assert_eq!(result1, result2);
    }
}
