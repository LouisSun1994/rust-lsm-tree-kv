# Q02：為什麼 MemTable 要有序，不能用 HashMap？

> 這題其實沒有用問句直接問出來，但程式碼讀到 `BTreeMap` 時必然會浮現。
> 這裡記下來，回顧時自己問自己。

## 提問

LSM 的寫入緩衝為什麼用 `BTreeMap`，不是更快的 `HashMap`？

## 結論

**因為 LSM 的所有後續運作都依賴「key 是有序的」這個前提。**

### HashMap 不行的三個原因

#### 原因 1：flush 出來的 SSTable 必須是有序的

```
HashMap dump:    {"x": 1, "a": 2, "m": 3}  → SSTable 是亂的
BTreeMap dump:   {"a": 2, "m": 3, "x": 1}  → SSTable 自然有序
```

如果 SSTable 是亂的，你只能：
- 整檔線性掃 → 讀取 O(n)
- 額外排序一次 → 浪費 CPU 與記憶體

#### 原因 2：SSTable 上要做 binary search

讀取一個 key 時，SSTable 內必須能 binary search 找到位置（O(log n)）。
這要求**檔案內 key 是排序的**。

#### 原因 3：Compaction 必須能歸併

Compaction 的核心是「k 路歸併」多個 SSTable 成一個。歸併排序的前提是
**輸入本身已經有序**。如果每個 SSTable 內部都是亂的，compaction 變成
「全部讀進記憶體 + 排序 + 寫出」 —— 對大檔來說根本做不到。

### 為什麼 BTreeMap 而不是 SkipList？

- 程式碼簡單、標準庫內建
- 對單執行緒場景效能足夠

工業級實作（RocksDB / LevelDB）用 SkipList 是因為它支援 lock-free 並發寫入，
這是這個學習專案還沒做到的部分。

## 對應的程式碼

[src/memtable.rs](../src/memtable.rs) 第 21 行：
```rust
use std::collections::BTreeMap;
```

如果改成 HashMap，整個 LSM 就會壞掉 —— 不是「變慢」，是根本無法運作。
這是個「決定性」的選擇，不是優化。

## 我學到了什麼

「資料結構選擇」往往不是看誰快，而是看**它要嵌入的更大系統需要什麼性質**。
HashMap 寫入是 O(1)、BTreeMap 是 O(log n)，但在 LSM 的脈絡下，
HashMap 的 O(1) 完全沒意義 —— 因為你之後要付 O(n) 的代價排序。
