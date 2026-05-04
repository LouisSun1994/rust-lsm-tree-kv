//! WAL：Write-Ahead Log（預寫日誌）
//!
//! # 為什麼需要 WAL？
//!
//! MemTable 在 RAM 裡，斷電就消失。如果寫入只到 MemTable 就告訴使用者「成功」，
//! 那一旦斷電，已確認的資料會遺失，違反「持久性」（Durability，ACID 的 D）。
//!
//! 解法：每次寫入時，先「append」一筆記錄到磁碟上的 WAL 檔，再更新 MemTable。
//! 重啟時，把 WAL 從頭重播一遍，就能還原 MemTable 的內容。
//!
//! # Append-only 的價值
//!
//! WAL 只 append、永不修改，所以磁碟可以用最快的「順序寫入」模式。
//! HDD 順序寫比隨機寫快幾百倍；SSD 也對順序寫比較友善（減少 write amplification）。
//! 這是 LSM-Tree 整體寫入快的根本原因。
//!
//! # 記錄格式（每筆寫入）
//!
//! ```text
//!  ┌────────┬─────────┬────────────┬────────┬──────┬──────────────┐
//!  │ crc32  │ key_len │ value_len  │ op_tag │ key  │ value (可空) │
//!  │ 4 byte │ 4 byte  │ 4 byte     │ 1 byte │ ...  │ ...          │
//!  └────────┴─────────┴────────────┴────────┴──────┴──────────────┘
//! ```
//!
//! - `op_tag`：0 = Put，1 = Delete（tombstone，沒有 value）。
//! - `crc32`：對 `key_len..value` 整段算 checksum。重播時校驗，遇到損壞就停下。
//!   這在「寫到一半斷電」的情境下特別重要 —— 最後一筆可能不完整。
//!
//! 所有整數用 little-endian 編碼（x86/ARM 原生序，省去轉換）。

use crate::memtable::MemTable;
#[cfg(test)]
use crate::memtable::Value;
use crate::Result;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const OP_PUT: u8 = 0;
const OP_DEL: u8 = 1;

pub struct Wal {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl Wal {
    /// 開啟（或建立）一個 WAL 檔，後續寫入會 append 到結尾。
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            writer: BufWriter::new(file),
        })
    }

    /// 寫入一筆 Put 紀錄。
    pub fn append_put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.append_record(OP_PUT, key, value)
    }

    /// 寫入一筆 Delete 紀錄（tombstone）。
    pub fn append_delete(&mut self, key: &[u8]) -> Result<()> {
        self.append_record(OP_DEL, key, &[])
    }

    fn append_record(&mut self, op: u8, key: &[u8], value: &[u8]) -> Result<()> {
        // 先把 payload（除 crc 以外的全部）組起來，方便算 checksum。
        let mut payload = Vec::with_capacity(4 + 4 + 1 + key.len() + value.len());
        payload.extend_from_slice(&(key.len() as u32).to_le_bytes());
        payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
        payload.push(op);
        payload.extend_from_slice(key);
        payload.extend_from_slice(value);

        let crc = crc32fast::hash(&payload);
        self.writer.write_all(&crc.to_le_bytes())?;
        self.writer.write_all(&payload)?;
        Ok(())
    }

    /// 確保 buffer 寫到 OS，並要求 OS 把檔案系統 cache 同步到磁碟。
    /// `fsync` 是「真的持久化」的關鍵 —— 只 flush BufWriter 還不夠，那只把資料交給 OS。
    /// 在 macOS 上 `sync_all` 內部會走 `F_FULLFSYNC` 嗎？實際上不會，標準庫只發
    /// `fsync(2)`；要對抗 macOS 的磁碟 cache 需要額外處理，這裡先簡化。
    pub fn sync(&mut self) -> Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 重播 WAL，把所有有效紀錄套用到一個新建的 MemTable。
    /// 遇到 crc 不對 / 檔案截斷時，視為「最後一筆寫到一半」，停在那裡（這是 WAL 的標準作法）。
    pub fn replay<P: AsRef<Path>>(path: P) -> Result<MemTable> {
        let mut mt = MemTable::new();
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(mt),
            Err(e) => return Err(e.into()),
        };
        let total_len = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let mut pos: u64 = 0;

        loop {
            if pos == total_len {
                break;
            }

            let mut crc_buf = [0u8; 4];
            if !read_full_or_eof(&mut reader, &mut crc_buf)? {
                break;
            }
            let expected_crc = u32::from_le_bytes(crc_buf);

            let mut header = [0u8; 9]; // key_len(4) + value_len(4) + op(1)
            if !read_full_or_eof(&mut reader, &mut header)? {
                break;
            }
            let key_len = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
            let value_len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let op = header[8];

            let mut key = vec![0u8; key_len];
            if !read_full_or_eof(&mut reader, &mut key)? {
                break;
            }
            let mut value = vec![0u8; value_len];
            if value_len > 0 && !read_full_or_eof(&mut reader, &mut value)? {
                break;
            }

            // 重新組 payload 算 crc，與檔案上的比對。
            let mut payload = Vec::with_capacity(9 + key_len + value_len);
            payload.extend_from_slice(&header);
            payload.extend_from_slice(&key);
            payload.extend_from_slice(&value);
            let actual_crc = crc32fast::hash(&payload);
            if actual_crc != expected_crc {
                // 損壞：把這筆之後的全部丟掉。實務上會把檔案 truncate 到 pos。
                break;
            }

            match op {
                OP_PUT => mt.put(key, value),
                OP_DEL => mt.delete(key),
                _ => break, // 未知 op，視為損壞
            }

            pos += 4 + 9 + key_len as u64 + value_len as u64;
        }

        Ok(mt)
    }

    /// 截斷 WAL（flush 完成後呼叫，把舊紀錄丟掉）。
    pub fn truncate(&mut self) -> Result<()> {
        self.writer.flush()?;
        let file = self.writer.get_mut();
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

/// 試著讀滿 buf。完整讀到 → true；一個 byte 都讀不到（乾淨 EOF）→ false；
/// 讀到一半 EOF（torn write）→ false。呼叫端只要看到 false 就停止重播。
fn read_full_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..])? {
            0 => return Ok(false),
            n => filled += n,
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_and_replay_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.log");

        {
            let mut wal = Wal::open(&path).unwrap();
            wal.append_put(b"a", b"1").unwrap();
            wal.append_put(b"b", b"2").unwrap();
            wal.append_delete(b"a").unwrap();
            wal.sync().unwrap();
        }

        let mt = Wal::replay(&path).unwrap();
        assert_eq!(mt.get(b"a"), Some(&Value::Tombstone));
        assert_eq!(mt.get(b"b"), Some(&Value::Put(b"2".to_vec())));
    }

    #[test]
    fn replay_handles_torn_tail() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.log");

        {
            let mut wal = Wal::open(&path).unwrap();
            wal.append_put(b"k1", b"v1").unwrap();
            wal.append_put(b"k2", b"v2").unwrap();
            wal.sync().unwrap();
        }

        // 模擬寫到一半斷電：把檔案最後幾個 byte 砍掉。
        let len = std::fs::metadata(&path).unwrap().len();
        let f = OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(len - 3).unwrap();

        let mt = Wal::replay(&path).unwrap();
        // 最後一筆損壞，但前面的 k1=v1 應該還在。
        assert_eq!(mt.get(b"k1"), Some(&Value::Put(b"v1".to_vec())));
        assert_eq!(mt.get(b"k2"), None); // 損壞那筆被丟棄
    }
}
