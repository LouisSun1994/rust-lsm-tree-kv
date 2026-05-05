# Cargo Cheatsheet

> Cargo = Rust 的官方建置工具 + 套件管理器 + 測試框架 + 文件產生器。
> 這份是給「會寫程式但剛碰 Rust」的人用的速查表。

## 一句話總結

> **Cargo 是 Rust 生態的單一入口 —— 學會它，等於學會 Rust 的整個 dev workflow。**

對照其他語言：

| 角色 | 對應 |
|---|---|
| Build tool | Make / Gradle / webpack |
| Package manager | npm / pip / Maven |
| Test runner | jest / pytest / go test |
| Doc generator | jsdoc / sphinx |
| Publish 工具 | npm publish / pypi upload |

裝 Rust 就一起裝了，整個社群只有這一套。

---

## 兩個核心檔案

### `Cargo.toml`（你寫的）

對應 `package.json`。看這專案的 [Cargo.toml](../Cargo.toml)：

```toml
[package]
name = "lsm_kv"
version = "0.1.0"
edition = "2021"            # Rust edition：2015/2018/2021/2024

[dependencies]
crc32fast = "1.4"           # 正式相依

[dev-dependencies]
tempfile = "3.10"           # 只有 cargo test 時用

[[bin]]                     # 額外的 binary
name = "lsm_kv"
path = "src/main.rs"

[profile.release]
opt-level = 3
lto = true
```

### `Cargo.lock`（自動產生）

對應 `package-lock.json` / `yarn.lock`。鎖定確切版本讓 build 可重現。

**規則**：
- **Binary crate**（有 main.rs）→ **要 commit**
- **Library crate**（只有 lib.rs）→ **不要 commit**

這專案兩種都有，優先當 binary 處理 → commit 是對的。

---

## 最常用指令（90% 的日常）

```bash
# 寫 code 時
cargo check                # 只做型別檢查（最快）
cargo test                 # 跑所有 #[test]
cargo clippy               # linter，比編譯器更挑剔
cargo fmt                  # 自動格式化

# 真要跑時
cargo build                # debug 模式
cargo build --release      # 最佳化模式（慢編譯快執行）
cargo run --release        # build + 執行
cargo run -- arg1 arg2     # `--` 後是傳給程式的參數

# 文件
cargo doc --open           # 從 ///、//! 註解產 HTML 文件並開瀏覽器
cargo doc --no-deps        # 不要把依賴的文件也產出（節省時間）

# 套件管理
cargo add <crate>          # 加依賴
cargo add <crate> --dev    # 加 dev-dependency
cargo remove <crate>       # 移除
cargo update               # 在 SemVer 範圍內更新 Cargo.lock
cargo tree                 # 看依賴樹

# 維護
cargo clean                # 清掉 target/（會大到 GB 級）
```

---

## 你在這專案實際用過的

```bash
cargo init                                              # 建專案（已做）
cargo build                                             # 編譯
cargo test                                              # 16 個測試全過
cargo run --release -- ./data put name Louis            # CLI put
cargo run --release -- ./data get name                  # CLI get
cargo run --release -- ./data bench 50000               # 跑 benchmark
```

---

## 概念辨析：Crate / Package / Workspace

| 名詞 | 定義 |
|---|---|
| **Crate** | 編譯單位。一個 .rs 樹（library 或 binary）|
| **Package** | 一個有 Cargo.toml 的目錄，**可包含多個 crate**（最多 1 個 lib + N 個 bin）|
| **Workspace** | 一組 package，共享 Cargo.lock 與 target/ |

這專案 = 1 個 package = 1 個 lib crate（lib.rs）+ 1 個 bin crate（main.rs）。

Workspace 範例（大型專案）：
```toml
[workspace]
members = ["engine", "cli", "server"]
```

TiKV、Servo、RocksDB binding 都是這樣組織的。

---

## 版本號規則（SemVer + Rust 特殊規則）

```toml
[dependencies]
crc32fast = "1.4"      # 等同 ^1.4，相容 1.4.0 ~ 1.x.x（不含 2.0）
serde = "1"            # 1.x.x
clap = "=4.3.2"        # 鎖死 4.3.2
tokio = "~1.36"        # 1.36.x（不跨 minor）
foo = "*"              # 任何版本（不建議）
```

### Rust 特殊規則：0.x 版本

```toml
foo = "0.5"            # 等同 ^0.5，相容 0.5.0 ~ 0.5.x（不含 0.6）
```

**0.x 版本下，minor bump 算 breaking change**。這跟 npm 的習慣不同。

---

## `target/` 目錄

```
target/
├── debug/             # cargo build 產物
├── release/           # cargo build --release 產物
└── doc/               # cargo doc 產物
```

`.gitignore` 預設排除 `/target`（這專案也是）。會大到 GB 級，定期 `cargo clean`。

---

## 進階：你以後會用到的

### Feature flags（條件編譯）

```toml
[features]
default = ["bloom"]
bloom = []
compaction = ["dep:rayon"]

[dependencies]
rayon = { version = "1", optional = true }
```

```bash
cargo build --no-default-features
cargo build --features compaction
cargo build --all-features
```

未來做 Bloom Filter 進階版時可以這樣切。

### 從 Git 直接拉相依

```toml
[dependencies]
rocksdb = { git = "https://github.com/rust-rocksdb/rust-rocksdb", branch = "master" }
my_local = { path = "../my_local_crate" }
```

### 個人 / 專案層設定（`.cargo/config.toml`）

```toml
[build]
jobs = 8                      # 平行編譯數
target-dir = "/tmp/target"    # target 放 tmpfs 加快

[net]
git-fetch-with-cli = true     # 用系統 git（解決 proxy 問題）
```

### Cargo plugins

```bash
cargo install cargo-watch     # cargo watch -x test：檔案變化自動跑測試
cargo install cargo-edit      # 提供 cargo add/rm/upgrade
cargo install cargo-expand    # 看 macro 展開後的 code
cargo install cargo-flamegraph # 產生火焰圖
cargo install cargo-audit     # 檢查相依有無 CVE
```

任何 `cargo-*` binary 都能當 `cargo *` 子命令。

---

## 為什麼 Cargo 評價這麼高

| 痛點 | 在哪些生態 | Cargo 怎麼解 |
|---|---|---|
| 多個套件管理器互打 | JS（npm/yarn/pnpm/bun）、Python（pip/poetry/uv）| 只有一個 |
| build / test / lint 各自工具 | C++、Java | 全包在 cargo 子命令 |
| 版本衝突 | Python | Cargo.lock 確定性 build |
| 跨平台 build 配置 | C/C++ | 自動處理 |

**設計哲學**：把 dev experience 當第一公民，不是事後補丁。
這在語言設計上是奢侈品 —— Python / JS 開發者花在 tooling 的時間，Rust 開發者完全省下來。

---

## 推薦立刻試的指令

針對這個專案，跑一次看看：

```bash
cd /Users/admin/sayyo/rust-lsm-tree-kv

cargo tree                              # 看 LSM 引擎依賴了什麼
cargo build --release --timings         # 看編譯每 crate 花多久
open target/cargo-timings/cargo-timing.html

cargo clippy                            # 看有什麼可以改善
cargo doc --no-deps --open              # 把自己寫的 //! /// 註解渲染成 HTML
```

最後那個特別值得試 —— 你會看到 src/ 裡寫的中文註解被渲染成漂亮的 HTML 文件。
等於 docs/ 之外多一份「從程式碼角度」的視角。
