//! LsmEngine：把 MemTable + WAL + SSTable 組合成可用的 KV 儲存。
//!
//! # 寫入路徑（put / delete）
//!
//! 1. Append 到 WAL（fsync 保證持久化）。
//! 2. 套用到 MemTable。
//! 3. 若 MemTable 大小 ≥ 門檻，flush 成 SSTable，然後清空 MemTable + 截斷 WAL。
//!
//! # 讀取路徑（get）
//!
//! 由「新」往「舊」找：
//! 1. MemTable
//! 2. SSTable 由新到舊（檔名數字越大越新）
//!
//! 任一層找到（包含 tombstone）就停。看到 tombstone 回傳「不存在」。
//!
//! # 還沒做的（學完這版可以接著挑戰）
//!
//! - **Compaction**：SSTable 越積越多會拖慢讀取。需要定期把多個小檔合併成大檔，
//!   過程中丟掉舊版本與 tombstone。可實作 Leveled 或 Tiered 策略。
//! - **Bloom Filter**：每個 SSTable 配一個 bloom，查 key 前先問 bloom，
//!   減少對「key 一定不存在」的檔案做 IO。
//! - **並發**：目前所有方法 `&mut self`。實務會把 MemTable 換成 lock-free skip list、
//!   並用「不可變 SSTable + 原子換版本」支援多讀者。
//! - **Manifest**：用一個獨立檔記錄「目前有效的 SSTable 列表」，避免崩潰時靠掃目錄推斷。

use crate::memtable::{MemTable, Value};
use crate::sstable::{SsTableReader, SsTableWriter};
use crate::wal::Wal;
use crate::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// MemTable 達到這個大小就 flush。預設 1 MiB（教學專案，故意設小，方便看到 flush 行為）。
const DEFAULT_MEMTABLE_FLUSH_BYTES: usize = 1 * 1024 * 1024;

pub struct LsmEngine {
    dir: PathBuf,
    memtable: MemTable,
    wal: Wal,
    /// 由「最舊」到「最新」存放。讀取時要從尾巴往前找。
    sstables: Vec<SsTableReader>,
    /// 下一個 SSTable 用的編號。每 flush 一次 +1。
    next_sst_id: u64,
    flush_threshold: usize,
}

impl LsmEngine {
    pub fn open<P: AsRef<Path>>(dir: P) -> Result<Self> {
        Self::open_with_threshold(dir, DEFAULT_MEMTABLE_FLUSH_BYTES)
    }

    pub fn open_with_threshold<P: AsRef<Path>>(dir: P, flush_threshold: usize) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        // 1. 掃目錄找出所有 SSTable，依編號排序。
        let mut sst_paths: Vec<(u64, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if let Some(rest) = name.strip_suffix(".sst") {
                    if let Ok(id) = rest.parse::<u64>() {
                        sst_paths.push((id, path));
                    }
                }
            }
        }
        sst_paths.sort_by_key(|(id, _)| *id);

        let next_sst_id = sst_paths.last().map(|(id, _)| id + 1).unwrap_or(0);
        let sstables = sst_paths
            .into_iter()
            .map(|(_, p)| SsTableReader::open(p))
            .collect::<Result<Vec<_>>>()?;

        // 2. 從 WAL 重建 MemTable。
        let wal_path = dir.join("wal.log");
        let memtable = Wal::replay(&wal_path)?;
        let wal = Wal::open(&wal_path)?;

        Ok(Self {
            dir,
            memtable,
            wal,
            sstables,
            next_sst_id,
            flush_threshold,
        })
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.wal.append_put(key, value)?;
        self.wal.sync()?;
        self.memtable.put(key.to_vec(), value.to_vec());
        self.maybe_flush()?;
        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.wal.append_delete(key)?;
        self.wal.sync()?;
        self.memtable.delete(key.to_vec());
        self.maybe_flush()?;
        Ok(())
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 先查 MemTable。
        if let Some(v) = self.memtable.get(key) {
            return Ok(materialize(v.clone()));
        }
        // 2. 再從最新到最舊查 SSTable。
        for sst in self.sstables.iter().rev() {
            if let Some(v) = sst.get(key)? {
                return Ok(materialize(v));
            }
        }
        Ok(None)
    }

    /// 強制把目前 MemTable flush 成 SSTable（測試與 demo 用）。
    pub fn flush(&mut self) -> Result<()> {
        if self.memtable.is_empty() {
            return Ok(());
        }
        self.do_flush()
    }

    fn maybe_flush(&mut self) -> Result<()> {
        if self.memtable.approximate_size() >= self.flush_threshold {
            self.do_flush()?;
        }
        Ok(())
    }

    fn do_flush(&mut self) -> Result<()> {
        let id = self.next_sst_id;
        let path = self.dir.join(format!("{:020}.sst", id));

        let mut writer = SsTableWriter::create(&path)?;
        for (k, v) in self.memtable.iter() {
            writer.append(k, v)?;
        }
        let path = writer.finish()?;

        // 開啟 reader 加進列表。
        let reader = SsTableReader::open(&path)?;
        self.sstables.push(reader);
        self.next_sst_id += 1;

        // SSTable 已經安全落地，現在可以重置 MemTable + WAL 了。
        // 順序很關鍵：先確認 SSTable 寫好（finish 內部已 fsync），再清 WAL。
        // 反過來做的話，若中途崩潰會丟資料。
        self.memtable = MemTable::new();
        self.wal.truncate()?;
        Ok(())
    }

    /// 給測試 / 觀察用：目前有幾個 SSTable。
    pub fn num_sstables(&self) -> usize {
        self.sstables.len()
    }
}

fn materialize(v: Value) -> Option<Vec<u8>> {
    match v {
        Value::Put(bytes) => Some(bytes),
        Value::Tombstone => None,
    }
}

// 雖然 Error::KeyNotFound 沒被 engine.rs 直接用，但保留它讓上層 API 將來可以用。
// 加 underscore 引用避免「unused variant」warning。
#[allow(dead_code)]
fn _ensure_error_used() -> Error {
    Error::KeyNotFound
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn put_get_basic() {
        let dir = tempdir().unwrap();
        let mut db = LsmEngine::open(dir.path()).unwrap();
        db.put(b"hello", b"world").unwrap();
        assert_eq!(db.get(b"hello").unwrap(), Some(b"world".to_vec()));
        assert_eq!(db.get(b"missing").unwrap(), None);
    }

    #[test]
    fn delete_hides_value() {
        let dir = tempdir().unwrap();
        let mut db = LsmEngine::open(dir.path()).unwrap();
        db.put(b"k", b"v").unwrap();
        db.delete(b"k").unwrap();
        assert_eq!(db.get(b"k").unwrap(), None);
    }

    #[test]
    fn survives_restart_via_wal() {
        let dir = tempdir().unwrap();
        {
            let mut db = LsmEngine::open(dir.path()).unwrap();
            db.put(b"persist", b"yes").unwrap();
            db.put(b"second", b"value").unwrap();
            // 故意不 flush，重啟時必須靠 WAL 還原。
        }
        let db = LsmEngine::open(dir.path()).unwrap();
        assert_eq!(db.get(b"persist").unwrap(), Some(b"yes".to_vec()));
        assert_eq!(db.get(b"second").unwrap(), Some(b"value".to_vec()));
    }

    #[test]
    fn flush_creates_sstable_and_reads_still_work() {
        let dir = tempdir().unwrap();
        let mut db = LsmEngine::open(dir.path()).unwrap();
        db.put(b"a", b"1").unwrap();
        db.put(b"b", b"2").unwrap();
        db.flush().unwrap();
        assert_eq!(db.num_sstables(), 1);
        assert_eq!(db.get(b"a").unwrap(), Some(b"1".to_vec()));
        // flush 後寫新值，應該蓋過 SSTable 裡的。
        db.put(b"a", b"1-new").unwrap();
        assert_eq!(db.get(b"a").unwrap(), Some(b"1-new".to_vec()));
    }

    #[test]
    fn newer_sstable_overrides_older() {
        let dir = tempdir().unwrap();
        let mut db = LsmEngine::open(dir.path()).unwrap();
        db.put(b"k", b"v1").unwrap();
        db.flush().unwrap();
        db.put(b"k", b"v2").unwrap();
        db.flush().unwrap();
        assert_eq!(db.num_sstables(), 2);
        assert_eq!(db.get(b"k").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn tombstone_in_memtable_hides_sstable_value() {
        let dir = tempdir().unwrap();
        let mut db = LsmEngine::open(dir.path()).unwrap();
        db.put(b"k", b"v").unwrap();
        db.flush().unwrap();
        db.delete(b"k").unwrap();
        assert_eq!(db.get(b"k").unwrap(), None);
    }

    #[test]
    fn restart_picks_up_existing_sstables() {
        let dir = tempdir().unwrap();
        {
            let mut db = LsmEngine::open(dir.path()).unwrap();
            db.put(b"x", b"1").unwrap();
            db.flush().unwrap();
            db.put(b"y", b"2").unwrap();
            db.flush().unwrap();
        }
        let db = LsmEngine::open(dir.path()).unwrap();
        assert_eq!(db.num_sstables(), 2);
        assert_eq!(db.get(b"x").unwrap(), Some(b"1".to_vec()));
        assert_eq!(db.get(b"y").unwrap(), Some(b"2".to_vec()));
    }

    #[test]
    fn auto_flush_triggers_when_threshold_exceeded() {
        let dir = tempdir().unwrap();
        // 把門檻設超低，確保第二筆寫入就觸發 flush。
        let mut db = LsmEngine::open_with_threshold(dir.path(), 8).unwrap();
        db.put(b"aaaa", b"bbbb").unwrap(); // 8 bytes, 達標 → flush
        assert!(db.num_sstables() >= 1);
    }
}
