use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use yakvdb::api::tree::Tree;
use yakvdb::api::{KV, Store};

pub struct Cache {
    dir: PathBuf,
    balance_db: KV,
    nonce_db: KV,
    storage_dbs: RwLock<HashMap<String, Arc<KV>>>,
}

impl Drop for Cache {
    fn drop(&mut self) {
        let _ = self.balance_db.flush();
        let _ = self.nonce_db.flush();
        let dbs = self.storage_dbs.read().unwrap();
        for db in dbs.values() {
            let _ = db.flush();
        }
    }
}

impl Cache {
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(dir)?;
        std::fs::create_dir_all(dir.join("storage"))?;
        std::fs::create_dir_all(dir.join("code"))?;
        std::fs::create_dir_all(dir.join("blocks"))?;
        std::fs::create_dir_all(dir.join("txs"))?;

        let balance_db = open_or_create_kv(&dir.join("balance.yak"))?;
        let nonce_db = open_or_create_kv(&dir.join("nonce.yak"))?;

        Ok(Self {
            dir: dir.to_path_buf(),
            balance_db,
            nonce_db,
            storage_dbs: RwLock::new(HashMap::new()),
        })
    }

    fn storage_db(&self, address: &str) -> anyhow::Result<Arc<KV>> {
        let addr = normalize_hex(address);
        {
            let dbs = self.storage_dbs.read().unwrap();
            if let Some(db) = dbs.get(&addr) {
                return Ok(Arc::clone(db));
            }
        }
        let path = self.dir.join("storage").join(format!("{addr}.yak"));
        let mut dbs = self.storage_dbs.write().unwrap();
        if let Some(db) = dbs.get(&addr) {
            return Ok(Arc::clone(db));
        } else {
            let db = Arc::new(open_or_create_kv(&path)?);
            dbs.insert(addr, Arc::clone(&db));
            return Ok(db);
        }
    }

    pub fn get_storage_at(
        &self,
        address: &str,
        slot: &str,
        block: &str,
    ) -> anyhow::Result<Option<String>> {
        let db = self.storage_db(address)?;
        let key = storage_key(slot, block);
        Ok(db.lookup(&key)?.map(|v| encode_data32(&v)))
    }

    pub fn put_storage_at(
        &self,
        address: &str,
        slot: &str,
        block: &str,
        value: &str,
    ) -> anyhow::Result<()> {
        let db = self.storage_db(address)?;
        let key = storage_key(slot, block);
        db.insert(&key, &pad32(value))?;
        Ok(())
    }

    pub fn get_balance(&self, address: &str, block: &str) -> anyhow::Result<Option<String>> {
        let key = address_block_key(address, block);
        Ok(self.balance_db.lookup(&key)?.map(|v| encode_quantity(&v)))
    }

    pub fn put_balance(&self, address: &str, block: &str, value: &str) -> anyhow::Result<()> {
        let key = address_block_key(address, block);
        self.balance_db.insert(&key, &decode_hex_compact(value))?;
        Ok(())
    }

    pub fn get_tx_count(&self, address: &str, block: &str) -> anyhow::Result<Option<String>> {
        let key = address_block_key(address, block);
        Ok(self.nonce_db.lookup(&key)?.map(|v| encode_quantity(&v)))
    }

    pub fn put_tx_count(&self, address: &str, block: &str, value: &str) -> anyhow::Result<()> {
        let key = address_block_key(address, block);
        self.nonce_db.insert(&key, &decode_hex_compact(value))?;
        Ok(())
    }

    pub fn get_code(&self, address: &str) -> anyhow::Result<Option<String>> {
        let addr = normalize_hex(address);
        let path = self.dir.join("code").join(format!("{addr}.bin.tgz"));
        read_gz_string(&path)
    }

    pub fn put_code(&self, address: &str, hex_code: &str) -> anyhow::Result<()> {
        let addr = normalize_hex(address);
        let path = self.dir.join("code").join(format!("{addr}.bin.tgz"));
        write_gz_string(&path, hex_code)
    }

    pub fn get_block_by_hash(&self, hash: &str) -> anyhow::Result<Option<String>> {
        let h = normalize_hex(hash);
        let path = self.dir.join("blocks").join(format!("{h}.json.tgz"));
        read_gz_string(&path)
    }

    pub fn put_block(&self, hash: &str, number: &str, json: &str) -> anyhow::Result<()> {
        let h = normalize_hex(hash);
        let blocks_dir = self.dir.join("blocks");
        let hash_path = blocks_dir.join(format!("{h}.json.tgz"));
        write_gz_string(&hash_path, json)?;

        let num = normalize_hex(number);
        let link_path = blocks_dir.join(&num);
        let _ = std::fs::remove_file(&link_path);
        #[cfg(unix)]
        std::os::unix::fs::symlink(format!("{h}.json.tgz"), &link_path)?;
        #[cfg(not(unix))]
        std::fs::copy(&hash_path, &link_path)?;

        Ok(())
    }

    pub fn get_block_by_number(&self, number: &str) -> anyhow::Result<Option<String>> {
        let num = normalize_hex(number);
        let path = self.dir.join("blocks").join(&num);
        read_gz_string(&path)
    }

    pub fn get_tx(&self, hash: &str) -> anyhow::Result<Option<String>> {
        let h = normalize_hex(hash);
        let path = self.dir.join("txs").join(format!("{h}.json.tgz"));
        read_gz_string(&path)
    }

    pub fn put_tx(&self, hash: &str, json: &str) -> anyhow::Result<()> {
        let h = normalize_hex(hash);
        let path = self.dir.join("txs").join(format!("{h}.json.tgz"));
        write_gz_string(&path, json)
    }

    pub fn get_receipt(&self, hash: &str) -> anyhow::Result<Option<String>> {
        let h = normalize_hex(hash);
        let path = self.dir.join("txs").join(format!("{h}.receipt.json.tgz"));
        read_gz_string(&path)
    }

    pub fn put_receipt(&self, hash: &str, json: &str) -> anyhow::Result<()> {
        let h = normalize_hex(hash);
        let path = self.dir.join("txs").join(format!("{h}.receipt.json.tgz"));
        write_gz_string(&path, json)
    }
}

fn storage_key(slot: &str, block: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(40);
    key.extend_from_slice(&pad32(slot));
    key.extend_from_slice(&pad8(block));
    key
}

fn address_block_key(address: &str, block: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(28);
    key.extend_from_slice(&pad20(address));
    key.extend_from_slice(&pad8(block));
    key
}

fn normalize_hex(s: &str) -> String {
    s.trim_start_matches("0x").to_lowercase()
}

fn decode_hex_padded<const N: usize>(hex_str: &str) -> [u8; N] {
    let clean = hex_str.trim_start_matches("0x");
    let padded = format!("{:0>width$}", clean, width = N * 2);
    let mut out = [0u8; N];
    hex::decode_to_slice(&padded[..N * 2], &mut out).unwrap_or_default();
    out
}

fn pad32(hex_str: &str) -> [u8; 32] {
    decode_hex_padded::<32>(hex_str)
}

fn pad20(hex_str: &str) -> [u8; 20] {
    decode_hex_padded::<20>(hex_str)
}

fn pad8(hex_str: &str) -> [u8; 8] {
    decode_hex_padded::<8>(hex_str)
}

fn read_gz_string(path: &Path) -> anyhow::Result<Option<String>> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut decoder = GzDecoder::new(&data[..]);
    let mut out = String::new();
    decoder.read_to_string(&mut out)?;
    Ok(Some(out))
}

fn write_gz_string(path: &Path, content: &str) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    let file = std::fs::File::create(&tmp)?;
    let mut encoder = GzEncoder::new(file, Compression::fast());
    encoder.write_all(content.as_bytes())?;
    encoder.finish()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn decode_hex_compact(s: &str) -> Vec<u8> {
    let clean = s.trim_start_matches("0x");
    if clean.is_empty() {
        return vec![0];
    }
    let even = if clean.len() % 2 != 0 {
        format!("0{clean}")
    } else {
        clean.to_string()
    };
    hex::decode(&even).unwrap_or_else(|_| vec![0])
}

fn encode_quantity(bytes: &[u8]) -> String {
    let trimmed = hex::encode(bytes);
    let trimmed = trimmed.trim_start_matches('0');
    if trimmed.is_empty() {
        "0x0".to_string()
    } else {
        format!("0x{trimmed}")
    }
}

fn encode_data32(bytes: &[u8]) -> String {
    format!("0x{:0>64}", hex::encode(bytes))
}

fn open_or_create_kv(path: &Path) -> anyhow::Result<KV> {
    if path.exists() {
        Ok(KV::open(path)?)
    } else {
        Ok(KV::make(path, 4096)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &[(&str, &str, &str, &str)] = &[
        (
            "0x1f98431c8ad98523631ae4a59f267346ea31f984",
            "0x3",
            "0x17599f9",
            "0x0000000000000000000000005e74c9f42eed283bff3744fbd1889d398d40867d",
        ),
        (
            "0xf38521f130fccf29db1961597bc5d2b60f995f85",
            "0x1",
            "0x17599f9",
            "0x0000000000000000000000000d5cd355e2abeb8fb1552f56c965b867346d6721",
        ),
        (
            "0x5c69bee701ef814a2b6a3edd4b1652cb9cc5aa6f",
            "0x0",
            "0x17599f9",
            "0x000000000000000000000000f38521f130fccf29db1961597bc5d2b60f995f85",
        ),
    ];

    #[test]
    fn test_storage_roundtrip() {
        let dir = std::path::PathBuf::from("target/test-cache-unit");
        let _ = std::fs::remove_dir_all(&dir);
        let cache = Cache::open(&dir).unwrap();

        for (addr, slot, block, val) in CASES {
            cache
                .put_storage_at(addr, slot, block, val)
                .unwrap_or_else(|e| panic!("put failed for {addr} slot {slot}: {e}"));
            let got = cache
                .get_storage_at(addr, slot, block)
                .unwrap_or_else(|e| panic!("get failed for {addr} slot {slot}: {e}"));
            assert_eq!(
                got.as_deref(),
                Some(*val),
                "roundtrip failed addr={addr} slot={slot}"
            );
        }
    }

    #[test]
    fn test_storage_persistence() {
        let dir = std::path::PathBuf::from("target/test-cache-persist");
        let _ = std::fs::remove_dir_all(&dir);

        // write
        {
            let cache = Cache::open(&dir).unwrap();
            for (addr, slot, block, val) in CASES {
                cache
                    .put_storage_at(addr, slot, block, val)
                    .unwrap_or_else(|e| panic!("put failed: {e}"));
            }
        } // cache dropped here — simulates proxy restart

        // reopen and read
        {
            let cache = Cache::open(&dir).unwrap();
            for (addr, slot, block, val) in CASES {
                let got = cache
                    .get_storage_at(addr, slot, block)
                    .unwrap_or_else(|e| panic!("get failed: {e}"));
                assert_eq!(
                    got.as_deref(),
                    Some(*val),
                    "not persisted: addr={addr} slot={slot}"
                );
            }
        }
    }
}
