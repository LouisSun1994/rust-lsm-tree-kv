//! MemTable：寫入緩衝區
//!
//! LSM-Tree 的所有寫入都先進到記憶體中的 MemTable。當 MemTable 達到大小門檻，
//! 就會被「凍結」並 flush 到磁碟成為 SSTable。
//!
//! 為了讓 flush 出來的 SSTable 自然有序（方便後續的 binary search 與 compaction），
//! MemTable 必須是「有序」的資料結構。
//!
//! 工業級系統（RocksDB、LevelDB）會用 Skip List；為了把焦點放在 LSM 概念本身，
//! 這裡用 Rust 標準庫的 `BTreeMap`。語意完全足夠。
//!
//! # 重點觀念：Tombstone（墓碑）
//!
//! LSM-Tree 不能「就地刪除」舊資料 —— SSTable 是寫入磁碟後不可變的。
//! 刪除操作其實是寫一個特殊的「墓碑」標記。讀取時看到墓碑就回傳「不存在」。
//! 真正的清理發生在 compaction 階段。

use std::collections::BTreeMap;

/// 一筆資料的值。`Some(bytes)` 表示寫入，`None` 表示刪除（墓碑）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Put(Vec<u8>),
    Tombstone,
}

impl Value {
    pub fn encoded_size(&self) -> usize {
        match self {
            Value::Put(v) => v.len(),
            Value::Tombstone => 0,
        }
    }
}

pub struct MemTable {
    map: BTreeMap<Vec<u8>, Value>,
    /// 約略追蹤目前已用的位元組數，用來決定何時 flush。
    /// 注意：這只是 key + value 的長度總和，沒算 BTreeMap 內部的指標開銷，
    /// 但作為 flush 觸發條件已足夠精準。
    approximate_size: usize,
}

impl MemTable {
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            approximate_size: 0,
        }
    }

    /// 寫入或覆蓋一筆資料。
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let new_size = key.len() + value.len();
        let old_size = self
            .map
            .get(&key)
            .map(|v| key.len() + v.encoded_size())
            .unwrap_or(0);
        self.approximate_size = self.approximate_size + new_size - old_size;
        self.map.insert(key, Value::Put(value));
    }

    /// 刪除（寫入 tombstone）。
    pub fn delete(&mut self, key: Vec<u8>) {
        let old_size = self
            .map
            .get(&key)
            .map(|v| key.len() + v.encoded_size())
            .unwrap_or(0);
        let new_size = key.len();
        self.approximate_size = self.approximate_size + new_size - old_size;
        self.map.insert(key, Value::Tombstone);
    }

    /// 讀取一筆資料。
    /// - `Some(Value::Put(v))`：找到值。
    /// - `Some(Value::Tombstone)`：這個 key 在這層被刪除了，呼叫端應停止往下層找。
    /// - `None`：這層完全沒看過這個 key。
    pub fn get(&self, key: &[u8]) -> Option<&Value> {
        self.map.get(key)
    }

    pub fn approximate_size(&self) -> usize {
        self.approximate_size
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 依 key 順序走訪所有條目。flush 到 SSTable 時會用到。
    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &Value)> {
        self.map.iter()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

impl Default for MemTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_get() {
        let mut mt = MemTable::new();
        mt.put(b"hello".to_vec(), b"world".to_vec());
        assert_eq!(mt.get(b"hello"), Some(&Value::Put(b"world".to_vec())));
        assert_eq!(mt.get(b"missing"), None);
    }

    #[test]
    fn put_overwrite_updates_size() {
        let mut mt = MemTable::new();
        mt.put(b"k".to_vec(), b"v1".to_vec());
        let s1 = mt.approximate_size();
        mt.put(b"k".to_vec(), b"vv2".to_vec());
        assert_eq!(mt.approximate_size(), s1 + 1); // 多一個 byte
    }

    #[test]
    fn delete_writes_tombstone() {
        let mut mt = MemTable::new();
        mt.put(b"k".to_vec(), b"v".to_vec());
        mt.delete(b"k".to_vec());
        assert_eq!(mt.get(b"k"), Some(&Value::Tombstone));
    }

    #[test]
    fn iter_yields_sorted_order() {
        let mut mt = MemTable::new();
        mt.put(b"c".to_vec(), b"3".to_vec());
        mt.put(b"a".to_vec(), b"1".to_vec());
        mt.put(b"b".to_vec(), b"2".to_vec());
        let keys: Vec<&Vec<u8>> = mt.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec![&b"a".to_vec(), &b"b".to_vec(), &b"c".to_vec()]);
    }
}
