# Q05：LSM 查詢複雜度為何可控？Bloom Filter 為何可信？

## 我的提問（兩個串起來的疑問）

第一輪：
> LSM 為什麼可以成為底層框架，這個在查詢應該是非常有問題的，複雜度相當相當高。
> B+ tree 的設計是在寫入時候會有 i/o 查詢，但 LSM 這種就是為了快速寫而產生的 pattern。
> 但這查詢理論上是異常慢的，除非查詢使用多進程併行去減少時間複雜度。

第二輪：
> 1. 為什麼提到的是 log N 是因為每一個 table 內查詢都是二分法嗎？
> 2. 如果說是這樣當我查詢的 data 就是沒有在 disk 中呢？這應該是最大複雜度吧。
>    如何確認當 bloom filter 反饋沒有的時候，因為這是機率性，完全相信就是失能，
>    是用什麼手段或演算法避免這塊？

## 結論

**裸 LSM 查詢確實慢，但真實 LSM 靠 4 個救命手段把成本壓回 ≈ B+Tree。**
**Bloom Filter 的「不在」回答是演算法數學保證的，不是機率性的。**

---

## Part 1：為什麼是 `O(log N)` —— 因為 binary search

### SSTable 內部的查詢路徑

```
SSTable 結構：
┌────────── data ──────────┐
│ entry_0 ~ entry_N        │
├────────── index ─────────┤
│ (key_0, offset_0)         │  ← 排序好的 vector
│ (key_1, offset_1)         │
│ ...                       │
├────────── footer ────────┤
│ index_offset, num_entries│
└──────────────────────────┘

查詢流程：
1. open 時：把 index 整段讀進 RAM      (一次性)
2. 查詢時：
   a. 在 index 上 binary search        → O(log N)，純 RAM
   b. 拿到 offset，磁碟讀 entry        → 1 次 IO
```

對應程式碼 [../src/sstable.rs:166](../src/sstable.rs:166)：
```rust
let pos = self.index.binary_search_by(|(k, _)| k.as_slice().cmp(key));
```

### 工業級的差別：Block-based SSTable

RocksDB 把 SSTable 切成 4KB block，index 只記每個 block 的「第一個 key + offset」：

```
1. RAM 中對 block index binary search   → O(log B)
2. 讀那個 4KB block                     → 1 次 IO
3. block 內 binary search 或線性掃       → O(log E)
總和：O(log B + log E) = O(log N)
```

仍是 `O(log N)`，但**index 本身不用爆 RAM**。

### `log N` 增長有多慢

```
N = 1,000          → log N ≈ 10
N = 1,000,000      → log N ≈ 20
N = 1,000,000,000  → log N ≈ 30
```

10 億筆資料只要 30 次比較。**真正的成本不在 binary search，在「磁碟 IO 次數」。**
所以 LSM 優化的核心不是 `log N` 那段，而是「**要碰幾個 SSTable**」這段。

---

## Part 2：裸 LSM 查詢確實災難等級

```
查 key = "x"，最壞情況：
  for SSTable in (新 → 舊):
      binary search 那個 SSTable

成本 = O(L · log N)，L = SSTable 數量

1000 個 SSTable × 1M entries：
  → 1000 次磁碟 IO × 0.1ms/IO = 100ms
  
B+Tree 同樣的查詢：
  → 3~4 次 IO < 1ms
```

**這個直覺是對的 —— 但真實 LSM 不是裸跑的。**

---

## Part 3：4 個救命手段把查詢成本壓回 B+Tree 等級

### 救命手段 1：Bloom Filter（最關鍵）

每個 SSTable 配一個 bloom filter（幾 KB bit array）。

```
查 key 之前先問 bloom：「key 可能在嗎？」
  說「不在」  → 100% 不在，跳過，不做磁碟 IO
  說「可能在」→ 才真的查 SSTable
```

**效果（1000 SSTable + 1% 偽陽性率）**：
```
無 bloom：1000 次 IO
有 bloom：~10 次 IO（990 個被跳過 + ~10 個誤判 + 1 個真有）
減少 99% 的 IO
```

### 救命手段 2：Compaction（讓 L 受控）

「1000 個 SSTable」這前提本身就錯。Compaction 持續合併。

**Leveled Compaction（RocksDB 預設）的關鍵性質**：
```
L0：~4 個檔，key range 重疊
L1：key range 互不重疊 → 找一個 key 只查 1 個檔
L2：key range 互不重疊 → 1 個檔
...
L6：key range 互不重疊 → 1 個檔

最壞 IO：L0 (4) + L1~L6 (6) = 10 個 SSTable
```

1 TB 資料分到 7 層左右，**「1000 個」變成「10 個」**。
再加 Bloom 過濾 → 平均剩 1~2 個真要 IO。

### 救命手段 3：Block Cache

SSTable 內部 4KB block，常用的 block 快取在 RAM。
熱資料 = 80% 查詢命中 cache → 0 磁碟 IO，純 RAM。
這部分跟 B+Tree 沒差別。

### 救命手段 4：Index 在 RAM

SSTable 開檔時 index 全進 RAM。binary search 是純 RAM 操作。
**真正的磁碟 IO 只發生在「讀 data block」這一步。**

---

## Part 4：Bloom Filter 為什麼可信 —— 單向誤差

### 我原本以為 Bloom Filter 兩邊都有機率誤差，所以「完全相信就是失能」。
### 真相：誤差是**單向**的。

| Bloom 回答 | 真實狀態 | 可能嗎？|
|---|---|---|
| 「可能在」 | 真的在 | ✅ |
| 「可能在」 | 不在 | ⚠️ 偽陽性 |
| 「不在」 | 真的不在 | ✅ |
| 「不在」 | 真的在 | ❌ **數學上不可能** |

**「不在」的回答是 100% 可信的演算法保證**，不是機率。

### 為什麼？

```
插入 key = "x"：
  hash1("x") = 17  → 第 17 位設 1
  hash2("x") = 42  → 第 42 位設 1
  hash3("x") = 88  → 第 88 位設 1

查詢時檢查 17、42、88：
  任一位是 0 → "x" 絕對沒被插入過（因為若插過，那位一定是 1）
  全部是 1   → 可能插過，也可能其他 key 剛好點亮這幾位（偽陽性）
```

```rust
fn might_contain(key: &[u8]) -> bool {
    for hash_fn in HASH_FUNCTIONS {
        let bit = hash_fn(key) % bit_array.len();
        if !bit_array[bit] {
            return false;  // ← 一旦遇到 0，立刻 100% 確定「不在」
        }
    }
    true  // 全部是 1，可能在（也可能偽陽性）
}
```

### LSM 的場景剛好完美匹配

```
Bloom 說「不在」 → 100% 不在 → 跳過 ✅ 安全
Bloom 說「可能在」→ 去磁碟確認 → 真有就回傳，沒有就繼續找下個 SSTable

偽陽性 = 浪費一次 IO，**不會給錯答案**
偽陰性 = 會給錯答案，**但 Bloom Filter 保證沒有**
```

### 偽陽性率怎麼控制：用記憶體換

```
m = -n · ln(p) / (ln 2)²
```

| 每個 key 用幾 bit | 偽陽性率 |
|---|---|
| 8 | ~2% |
| 10 | ~1%（RocksDB 預設）|
| 16 | ~0.05% |
| 24 | ~0.001% |

1 億 key 用 1% 偽陽性，只要 ~120 MB —— 對伺服器是九牛一毛。

### Bloom Filter 與 LSM 的歷史巧合

Bloom Filter（1970）比 LSM（1996）早 26 年發明。
**LSM 是被設計來剛好契合 Bloom Filter 特性的** —— 把「key 不存在」這個常見路徑
交給「絕對正確」的方向，把「key 存在」交給「機率正確 + 後續驗證」的方向。

---

## Part 5：完整查詢成本

```
查 key = "x"：

1. MemTable binary search          → O(log n) RAM，幾十 ns
2. for each SSTable (新→舊):
     bloom 檢查                    → ~100 ns
     不在 → continue ✅
     可能在 → index binary search  → 幾百 ns RAM
            → data block read      → 100 µs（或 cache 命中 ~100 ns）

平均：< 1 次磁碟 IO
最壞：log(總資料) 次比較 + 幾次 IO（被 bloom + compaction 控制）
```

對比 B+Tree 平均 1~2 次 IO —— **同個數量級**。

---

## Part 6：點查不需要併行，但範圍查需要

我原本提到「除非用多進程併行」—— 這個直覺在點查上錯，但**在範圍查上對**。

### 點查（`get(key)`）
平均只查 1~2 個 SSTable，併行反而增加 context switch 開銷。

### 範圍查（`scan(start..end)`）
本來就要從多個 SSTable 拼回完整 key 序列：
```
每個 SSTable 一個 iterator
        │
        ▼
  min-heap 按 key 排序
        │
        ▼
  merged stream（同 key 取最新、丟墓碑）
```
RocksDB 的 `MergingIterator`，常用 thread pool 平行 prefetch 各 iterator 的下個 block 用滿磁碟頻寬。
**這不是「為了讓查詢變快」，而是「範圍查本來就要歸併」。**

---

## Part 7：LSM 為什麼能當底層框架（總結）

不是因為查詢快，而是 4 個工程理由：

1. **寫入快一個數量級，讀取沒慢一個數量級** → 整體交易划算
2. **對現代硬體友善**：SSD 怕 in-place update（NAND 壽命）、雲端硬碟 IOPS 有上限、HDD 順序快 200 倍
3. **P99 可預期**：B+Tree 有 page split 抖動，LSM 寫入路徑固定
4. **可調可控**：MemTable 大小、Compaction 策略、Bloom 大小、Block size、壓縮算法 —— 工程師大量旋鈕

### B+Tree 沒被淘汰的場景

| 場景 | 贏家 | 原因 |
|---|---|---|
| 高頻點查 + 低寫入 | B+Tree | 沒讀取放大 |
| 強事務（OLTP） | B+Tree | in-place update 對 ACID 直觀 |
| 小資料庫 (<1GB) | B+Tree | LSM compaction 開銷不划算 |

MySQL InnoDB / PostgreSQL 是 B+Tree 是有原因的。
但「寫入 > 10K ops/s」「資料量 > TB」「線上服務需 P99」的新系統，幾乎都選 LSM。

---

## 我學到了什麼

1. **複雜度直覺要區分「最壞」與「平均」**。LSM 最壞情況看起來災難，但 4 個工程手段把它壓到罕見。
   下次評估任何架構，要問：「最壞情況多常發生？緩解手段是什麼？」
2. **Bloom Filter 的單向誤差設計**是演算法美學的範例 ——
   把「絕對」與「機率」拆開，分別處理常見與罕見路徑。
   這個 idea 可以推廣到很多場景：cache 預檢、垃圾郵件預過濾、CDN edge 判斷。
3. **「機率性」≠「不可信」**。要看誤差方向跟使用情境是否匹配。
   Bloom 的偽陽性會被下游驗證修正，偽陰性會錯但被演算法保證沒有。
4. **架構評估不能只看 big-O**。`O(log N)` 跟 `O(L · log N)` 看起來只差一個常數，
   但在磁碟 IO 主導成本的世界裡，這個常數就是生死。
5. **真實系統都是「裸架構 + N 個救命手段」的疊加**。讀程式碼或 paper 時要分清楚，
   哪些是核心 idea，哪些是讓核心 idea 能上線的工程補丁。LSM 的核心很簡單，工程補丁很多。
