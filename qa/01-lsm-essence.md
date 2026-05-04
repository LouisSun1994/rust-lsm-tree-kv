# Q01：LSM 本質上只是 mem block + save disk？

## 我的提問

> 所以 LSM 本質上只是 一塊一塊的 mem block 然後 save disk !? 這樣？

## 結論：骨架對，但有 4 個細節讓它從玩具變成資料庫核心

### ✅ 你抓對的骨架

```
寫入 → 累積到 mem block (MemTable) → 滿了就存 disk (SSTable)
```

這就是 LSM 的核心 idea，一句話可以收尾。

### 但讓它能當資料庫底層的 4 個細節

#### 1. mem block 必須是**有序的**

不是隨便累積然後 dump，而是用 BTreeMap / SkipList 維持 key 的順序。

**為什麼？** 有序才能：
- 在 SSTable 上做 binary search（O(log n) 而不是 O(n)）
- 多個 SSTable 用「歸併」高效合併（compaction）

#### 2. 寫 disk 之前要先寫一份「保險」（WAL）

光是「累積到 mem 再寫 disk」會有致命問題：1000 筆都還在 RAM → 突然斷電 → 全消失。

所以**先 append 到磁碟上的 WAL**，再更新 MemTable。
WAL 只給「斷電重啟」用。

#### 3. SSTable **永遠不改**（不可變）

```
傳統思維：              LSM 思維：
key=x, value=1   →     SSTable_1: x=1
key=x, value=2   →     SSTable_1: x=1     (不動！)
                       SSTable_2: x=2     (新檔)
                       
讀 x → 找最新的 → SSTable_2 → 回 2
```

**好處**：不需要鎖、寫入永遠是 append、讀取無 race condition
**代價**：同一個 key 多版本散落在不同檔案 → 需要 compaction 整理

#### 4. 「刪除」不是真的刪，是寫**墓碑（Tombstone）**

SSTable 不可變，所以刪除只能寫一個「我死了」的標記。
讀取時看到墓碑就回「不存在」。
真正的清理發生在 compaction。

## 完整定義

> **LSM = 「有序 mem block」+「append-only WAL 保險」+「不可變的有序磁碟檔」+「定期 compaction 合併」**

少了任一項都會壞掉：

| 少了什麼 | 後果 |
|---|---|
| 有序 | 讀取 O(n)、無法做高效 compaction |
| WAL | 斷電丟資料 |
| 不可變 | 鎖、複雜度、寫入放大爆炸 |
| Compaction | 檔案越積越多 → 讀取越慢 |

## 我學到了什麼

LSM 的「形」很簡單，「神」在於四個約束的組合。
能把這 4 個說出口才算真的懂。
