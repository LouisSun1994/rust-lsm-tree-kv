//! SSTable：Sorted String Table（有序字串表）
//!
//! SSTable 是 LSM-Tree 在磁碟上的儲存單位。特性：
//! - **有序**：條目依 key 升序排列，方便 binary search 與 merge。
//! - **不可變**（immutable）：寫一次就不再修改，要刪只能整檔 unlink。
//!
//! 「不可變」帶來的好處：
//! - 不需要鎖（多執行緒讀完全安全）。
//! - 檔案系統 cache 命中率高。
//! - Compaction 邏輯簡單：讀取多個 SSTable + 寫一個新檔，舊檔最後再刪。
//!
//! # 本實作的格式（為了清晰，用最簡形式；不做 block-based）
//!
//! ```text
//!  ┌──────────── data section ────────────┐
//!  │ entry_0                                │
//!  │ entry_1                                │
//!  │ ...                                    │
//!  ├──────────── index section ────────────┤
//!  │ index_0  (key, file_offset_of_entry)   │
//!  │ index_1                                │
//!  │ ...                                    │
//!  ├──────────── footer (固定 16 bytes) ───┤
//!  │ index_offset (8) | num_entries (8)     │
//!  └────────────────────────────────────────┘
//! ```
//!
//! 每筆 entry：`crc32(4) | key_len(4) | value_len(4) | tag(1) | key | value`
//! - tag：0 = Put，1 = Tombstone（value_len 必為 0）
//!
//! 每筆 index：`key_len(4) | key | offset(8)`
//!
//! 讀取流程：
//! 1. 開檔，seek 到末尾讀 footer，拿到 index_offset。
//! 2. 把整個 index 讀進記憶體（用 binary search 找 key）。
//! 3. 找到後 seek 到 entry offset，讀那一筆。
//!
//! 工業實作（如 RocksDB）會：
//! - 把 data 切成 4KB 的 block，每 block 自帶 restart points 加速搜尋。
//! - 加 Bloom Filter 快速判斷「key 一定不在這檔裡」，避免無謂 IO。
//! - 把 index 也分塊，避免大檔的 index 吃太多 RAM。
//!
//! 這些優化都建立在這個基本架構上，先弄懂這版再延伸。

use crate::memtable::Value;
use crate::{Error, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const TAG_PUT: u8 = 0;
const TAG_TOMBSTONE: u8 = 1;
const FOOTER_SIZE: u64 = 16;

/// SSTable 寫入器：拿一個依 key 排序的迭代器，產生一個 SSTable 檔。
pub struct SsTableWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    /// 累積每筆 entry 的 (key, offset)，最後一次寫成 index。
    index: Vec<(Vec<u8>, u64)>,
    cursor: u64,
    last_key: Option<Vec<u8>>,
}

impl SsTableWriter {
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            writer: BufWriter::new(file),
            index: Vec::new(),
            cursor: 0,
            last_key: None,
        })
    }

    /// 寫入一筆 entry。輸入必須是「依 key 升序」的，且不能有重複 key。
    pub fn append(&mut self, key: &[u8], value: &Value) -> Result<()> {
        if let Some(prev) = &self.last_key {
            assert!(
                prev.as_slice() < key,
                "SsTableWriter::append 接收到的 key 必須嚴格遞增"
            );
        }

        let (tag, value_bytes): (u8, &[u8]) = match value {
            Value::Put(v) => (TAG_PUT, v.as_slice()),
            Value::Tombstone => (TAG_TOMBSTONE, &[]),
        };

        let mut payload = Vec::with_capacity(9 + key.len() + value_bytes.len());
        payload.extend_from_slice(&(key.len() as u32).to_le_bytes());
        payload.extend_from_slice(&(value_bytes.len() as u32).to_le_bytes());
        payload.push(tag);
        payload.extend_from_slice(key);
        payload.extend_from_slice(value_bytes);
        let crc = crc32fast::hash(&payload);

        let offset = self.cursor;
        self.writer.write_all(&crc.to_le_bytes())?;
        self.writer.write_all(&payload)?;
        self.cursor += 4 + payload.len() as u64;

        self.index.push((key.to_vec(), offset));
        self.last_key = Some(key.to_vec());
        Ok(())
    }

    /// 結束寫入：把 index 與 footer 寫進檔案、fsync、回傳檔案路徑。
    pub fn finish(mut self) -> Result<PathBuf> {
        let index_offset = self.cursor;
        let num_entries = self.index.len() as u64;

        for (key, offset) in &self.index {
            self.writer.write_all(&(key.len() as u32).to_le_bytes())?;
            self.writer.write_all(key)?;
            self.writer.write_all(&offset.to_le_bytes())?;
        }
        self.writer.write_all(&index_offset.to_le_bytes())?;
        self.writer.write_all(&num_entries.to_le_bytes())?;

        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(self.path)
    }
}

/// SSTable 讀取器。把 index 整段載入記憶體，data 段按需 seek-read。
pub struct SsTableReader {
    file: std::cell::RefCell<BufReader<File>>,
    index: Vec<(Vec<u8>, u64)>,
    path: PathBuf,
}

impl SsTableReader {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(&path)?;
        let file_len = file.metadata()?.len();
        if file_len < FOOTER_SIZE {
            return Err(Error::Corruption(format!(
                "SSTable 檔案 {:?} 太小，無法包含 footer",
                path.as_ref()
            )));
        }

        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(file_len - FOOTER_SIZE))?;
        let mut footer = [0u8; FOOTER_SIZE as usize];
        reader.read_exact(&mut footer)?;
        let index_offset = u64::from_le_bytes(footer[0..8].try_into().unwrap());
        let num_entries = u64::from_le_bytes(footer[8..16].try_into().unwrap()) as usize;

        if index_offset > file_len - FOOTER_SIZE {
            return Err(Error::Corruption("index_offset 超出檔案範圍".to_string()));
        }

        reader.seek(SeekFrom::Start(index_offset))?;
        let mut index = Vec::with_capacity(num_entries);
        for _ in 0..num_entries {
            let mut key_len_buf = [0u8; 4];
            reader.read_exact(&mut key_len_buf)?;
            let key_len = u32::from_le_bytes(key_len_buf) as usize;
            let mut key = vec![0u8; key_len];
            reader.read_exact(&mut key)?;
            let mut offset_buf = [0u8; 8];
            reader.read_exact(&mut offset_buf)?;
            index.push((key, u64::from_le_bytes(offset_buf)));
        }

        Ok(Self {
            file: std::cell::RefCell::new(reader),
            index,
            path: path.as_ref().to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn num_entries(&self) -> usize {
        self.index.len()
    }

    /// 在這個 SSTable 裡找 key。
    /// 返回 `Ok(Some(value))`：找到（包含 tombstone）。
    /// 返回 `Ok(None)`：這個 SSTable 沒有這個 key。
    pub fn get(&self, key: &[u8]) -> Result<Option<Value>> {
        // 在 index 上做 binary search。
        let pos = self.index.binary_search_by(|(k, _)| k.as_slice().cmp(key));
        let offset = match pos {
            Ok(i) => self.index[i].1,
            Err(_) => return Ok(None),
        };

        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(offset))?;

        let mut crc_buf = [0u8; 4];
        file.read_exact(&mut crc_buf)?;
        let expected_crc = u32::from_le_bytes(crc_buf);

        let mut header = [0u8; 9];
        file.read_exact(&mut header)?;
        let key_len = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
        let value_len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let tag = header[8];

        let mut key_buf = vec![0u8; key_len];
        file.read_exact(&mut key_buf)?;
        let mut value_buf = vec![0u8; value_len];
        if value_len > 0 {
            file.read_exact(&mut value_buf)?;
        }

        let mut payload = Vec::with_capacity(9 + key_len + value_len);
        payload.extend_from_slice(&header);
        payload.extend_from_slice(&key_buf);
        payload.extend_from_slice(&value_buf);
        let actual_crc = crc32fast::hash(&payload);
        if actual_crc != expected_crc {
            return Err(Error::Corruption(format!(
                "SSTable {:?} 在 offset {} 的 entry 校驗失敗",
                self.path, offset
            )));
        }

        // 防呆：index 指到的 entry 的 key 必須跟我們找的 key 相同。
        if key_buf != key {
            return Err(Error::Corruption(
                "SSTable index 與 entry 的 key 不一致".to_string(),
            ));
        }

        let value = match tag {
            TAG_PUT => Value::Put(value_buf),
            TAG_TOMBSTONE => Value::Tombstone,
            _ => {
                return Err(Error::Corruption(format!(
                    "SSTable 內未知 tag: {}",
                    tag
                )))
            }
        };
        Ok(Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("0.sst");

        {
            let mut w = SsTableWriter::create(&path).unwrap();
            w.append(b"apple", &Value::Put(b"red".to_vec())).unwrap();
            w.append(b"banana", &Value::Put(b"yellow".to_vec())).unwrap();
            w.append(b"cherry", &Value::Tombstone).unwrap();
            w.finish().unwrap();
        }

        let r = SsTableReader::open(&path).unwrap();
        assert_eq!(r.num_entries(), 3);
        assert_eq!(r.get(b"apple").unwrap(), Some(Value::Put(b"red".to_vec())));
        assert_eq!(
            r.get(b"banana").unwrap(),
            Some(Value::Put(b"yellow".to_vec()))
        );
        assert_eq!(r.get(b"cherry").unwrap(), Some(Value::Tombstone));
        assert_eq!(r.get(b"durian").unwrap(), None);
    }

    #[test]
    fn binary_search_finds_middle_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("0.sst");

        let mut w = SsTableWriter::create(&path).unwrap();
        for i in 0..1000u32 {
            let key = format!("key{:05}", i);
            let value = format!("value{}", i);
            w.append(key.as_bytes(), &Value::Put(value.into_bytes()))
                .unwrap();
        }
        w.finish().unwrap();

        let r = SsTableReader::open(&path).unwrap();
        assert_eq!(
            r.get(b"key00500").unwrap(),
            Some(Value::Put(b"value500".to_vec()))
        );
        assert_eq!(
            r.get(b"key00999").unwrap(),
            Some(Value::Put(b"value999".to_vec()))
        );
        assert_eq!(r.get(b"key99999").unwrap(), None);
    }
}
