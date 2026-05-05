# Q06：為什麼推薦我使用 Rust 而不是 C/C++？

## 我的提問

> 那為什麼是推薦我使用 rust 而不是 c/c++

## 結論

**C/C++ 沒過時，Rust 也不是時尚 —— 對「啟蒙學習者 + 想跨系統」的訴求，Rust 在五個維度上完勝：
編譯器當助教、Cargo 省 build 痛苦、性能不輸、找工作差異化、寫新東西用 Rust、讀舊程式碼仍學 C。**
這兩件事可以分開做，互補。

---

## 1. 對啟蒙學習者最關鍵的差別：錯誤回饋的時機

### C/C++ 路徑（沉默的、延遲的 bug）
```
寫 code → 編譯通過 → 跑起來看似正常 → 跑到邊界條件 → segfault
→ 用 gdb / valgrind / core dump 慢慢追
→ 兩天後才知道是哪行寫錯
→ 你以為自己笨
```

### Rust 路徑（編譯期當下提示）
```
寫 code → 編譯炸（前 1~2 週狂炸）
→ 編譯器告訴你「第 42 行 borrow 違規，建議這樣改」
→ 改完 → 通常就對了
→ 你學到「為什麼這樣寫不安全」
```

**對沒寫過系統語言的人，這個差距是天差地遠。**
跟 borrow checker 打架的過程 = 學系統程式設計概念的過程。
等於有個 24 小時不吃飯的助教在旁邊。

---

## 2. 同樣的坑，C/C++ 要你記住，Rust 編譯器幫你記

| 坑 | C/C++ | Rust |
|---|---|---|
| Use-after-free | 🔴 自己注意 | 🟢 編譯不過 |
| Double-free | 🔴 自己注意 | 🟢 編譯不過 |
| Buffer overflow | 🔴 自己注意 | 🟢 自動 bounds check |
| Null pointer dereference | 🔴 自己注意 | 🟢 沒 null，用 Option |
| Data race | 🔴 自己用鎖 | 🟢 編譯不過 |
| Integer overflow | 🔴 silent UB | 🟢 debug panic / release wrap |
| Iterator invalidation | 🔴 自己注意 | 🟢 編譯不過 |

### 真實數據
- **Microsoft**：自家產品 70% 的安全漏洞來自 memory safety 問題
- **Chrome**：類似比例
- **Android**：2019 後新元件改用 Rust → 這類漏洞**直接降到 0**

不是行銷，是有資料的。

---

## 3. C++ 比 C 更危險（反直覺）

很多人以為「C++ 比 C 進化所以較安全」 —— 錯。

```
C：簡單但危險。陷阱明顯，老手都認得。
C++：強大但更危險：
  - 多重繼承、virtual destructor、operator overloading、template metaprogramming
  - 隱式轉換爆炸：copy/move/conversion constructor、reference collapsing
  - 「這段做了什麼」往往要追 5 層 template
  - 編譯錯誤訊息惡名昭彰（一個錯印 200 行）
```

**「C++ 是個會走路的腳槍」是業界笑話。**
Rust 想做的就是「拿掉腳槍但保留 C++ 的零成本抽象」。

```cpp
// C++：哪些 copy？哪些 move？哪些 throw？看不出來
auto x = foo(bar(baz()));

// Rust：每個 move 都顯式，編譯器逼你想清楚
let x = foo(bar(baz()));
```

---

## 4. Cargo vs C/C++ build system

差距大到不可思議。

### C/C++ 蓋一個專案的真實流程
```
1. 選 build system：Make / CMake / Bazel / Meson / autotools / ninja？打架打 30 年
2. 抓相依：vcpkg / conan / apt / brew / hunter？header 在哪？.so 在哪？
3. 跨平台：Linux 編得過，Mac 編不過（不同 glibc）；Windows 又一套
4. 連結：動態 vs 靜態？LD_LIBRARY_PATH 設好沒？
5. 半天後 hello world 終於跑起來
```

### Rust 同樣的事
```
$ cargo new my-project
$ cargo add tokio
$ cargo run
# 跑起來了
```

**Cargo 把 C/C++ 30 年沒解決的問題一次解決。**
對學習者最大的好處：時間花在學 LSM-Tree，不是學「為什麼 CMake 找不到 boost」。

---

## 5. 性能 —— 通常一樣快，有時 Rust 更快

| 比較 | 結果 |
|---|---|
| Rust vs C++ 微基準（Benchmarks Game）| 大致打平 |
| Rust 沒 GC | 跟 C/C++ 同樣無 pause |
| Ownership 在編譯期解決 | runtime 沒額外成本 |
| 零成本抽象 | 跟 C++ 同設計目標 |
| Iterator 鏈 | 編譯後跟手寫 for loop 同等速度 |

**有時 Rust 更快**，因為：
- 編譯器知道更多保證（aliasing 規則嚴格）→ 可做 C 不敢做的優化
- LLVM 對 Rust 的 IR 比對 C++ 的更乾淨

實際案例：
- **Cloudflare** 把 Nginx 換成 Pingora（Rust）→ 效能更好
- **Microsoft** Windows kernel 開始用 Rust 重寫部分模組
- **Linux Kernel 6.1** 起接受 Rust（C 之外史上第二個官方語言）

這些團隊不是因為 Rust 潮才換，是工程上算過划算。

---

## 6. 職涯加成 —— 系統領域哪些團隊在用 Rust

| 公司 / 專案 | 用途 |
|---|---|
| Microsoft | Windows kernel 部分模組、Azure 基礎設施 |
| Google | Android 系統元件、Fuchsia OS |
| Amazon | Firecracker（serverless microVM）、Bottlerocket OS |
| Meta | Buck2 build system |
| Linux Kernel | 第二官方語言 |
| Cloudflare | Pingora（取代 Nginx） |
| Discord | 從 Go 換 Rust 後 latency 改善 50x |
| 資料庫 | TiKV、Materialize、Databend、Qdrant、SurrealDB |
| 區塊鏈 | Solana、Polkadot、Aptos、Sui |
| 嵌入式 | ESP32 官方支援 |

**會 Rust 寫系統程式現在是稀缺技能。**
會 C/C++ 的人多到爆，會 Rust 的少很多 —— 履歷差異化角度 Rust > C++。

而且：**會 Rust 的人 99% 讀得懂 C/C++（觀念可遷移），反過來不一定。**

---

## 7. 什麼時候反過來推薦 C？

公平起見，這些情況「先學 C 不是 Rust」更合理：

- **OS kernel / driver / 嵌入式 MCU**：要跟既有 C ABI 互動
- **超低資源設備**（< 32KB RAM）：Rust 生態比較單薄
- **維護既有 C/C++ codebase**：公司產品就是 C 寫的
- **想 contribute Linux / Postgres / Redis**：這些都是 C
- **想極致理解電腦底層**：C 抽象少，看到的就是接近機器的東西
- **演算法面試 / 系所作業系統課**：多半用 C/C++

**精準建議**：**讀寫用 Rust，花時間讀懂 C（不一定寫好）。**
兩者搭配 = 既會用現代工具產出，也看得懂歷史經典。

---

## 8. 對我個人的具體建議

| 因素 | C/C++ | Rust |
|---|---|---|
| 學習曲線 | 平緩但坑深（以為懂，其實沒） | 陡但坑淺（編譯器逼你懂） |
| 寫產出速度（學會後） | 中 | 中 |
| 出 bug 機率 | 高（不是天天寫） | 低 |
| 找工作差異化 | 一般 | 高 |
| 學完能跨應用領域 | 系統 / 嵌入式 | 系統 / 雲端 / 區塊鏈 / WASM |
| 配套工具痛苦度 | 高 | 低 |

對「啟蒙 + 想換跑道」的訴求，Rust 完勝。
**C/C++ 的學習價值是「讀懂老 codebase」，不是「寫新專案」。** 這兩件事可分開，不衝突。

---

## 9. 推薦的學習順序

```
階段 1：用 Rust 寫東西（現在這裡）
  - 完成 LSM-Tree
  - 加 Bloom Filter、Compaction
  - 寫 1~2 個能放履歷的中型專案

階段 2：補 C 的閱讀能力
  - 讀 LevelDB 原始碼（C++）
  - 讀 Redis 原始碼（C）
  - 不一定要寫，能看懂、能 trace bug 就夠

階段 3：依需求決定
  - 進系統廠寫 driver → C
  - 進資料庫公司 → Rust 或 C++ 看公司
  - 嵌入式 → 看 chip 廠商，多半 C
```

---

## 一張圖收尾

```
           「想學系統程式」
                  │
         ┌────────┴────────┐
         │                 │
    產出新東西          讀懂舊東西
         │                 │
         ▼                 ▼
       Rust              C / C++
   （安全、效率、       （經典、龐大、
     工具完善）          無處不在）
         │                 │
         └────────┬────────┘
                  ▼
           寫 Rust 為主，
           讀 C 為輔。
        兩種能力互相加分。
```

---

## 我學到了什麼

1. **「該學 X 還是 Y」這類問題沒有絕對答案，要看「目的」與「階段」**。
   對「啟蒙 + 跨系統」的階段，Rust 對我有具體優勢；對「維護 Linux kernel」的場景，C 仍是答案。

2. **學語言不只看語言本身，要看整個生態（工具鏈、社群、典範案例）**。
   Cargo / crates.io / 編譯器訊息品質這些「周邊」對學習速度的影響，可能比語言特性還大。

3. **「會寫」與「會讀」是兩種能力，可以分開培養**。
   寫 Rust 累積產出，讀 C 累積對歷史經典的理解。兩件事不互斥。

4. **產業選 Rust 的真實理由是工程經濟學**（記憶體 bug 帶來的安全成本 + 開發者時間成本），
   不是「語言比較潮」。看到 Microsoft / Google / Amazon / Linux 都在用，這是強訊號。

5. **C++ 的複雜度是負債，不是資產**。「學 C++ 比 C 進化」這種直覺反而會誤導 ——
   C++ 的隱式行為比 C 多，新手踩雷機率更高。要選底層歷史經典，學 C 不學 C++。
