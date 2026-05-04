//! LSM-Tree Key-Value 儲存引擎
//!
//! 這個 crate 實作一個簡化版的 LSM-Tree（Log-Structured Merge Tree）儲存引擎。
//! 主要模組：
//! - [`memtable`]：記憶體中的有序資料結構（寫入緩衝）
//! - [`wal`]：Write-Ahead Log，崩潰回復用
//! - [`sstable`]：磁碟上的不可變排序字串表
//! - [`engine`]：將上述組件組合起來的對外介面

pub mod memtable;
pub mod wal;
pub mod sstable;
pub mod engine;

pub use engine::LsmEngine;

/// 統一的錯誤型別。實務上會用 `thiserror` 之類的 crate，
/// 為了減少依賴，這裡手寫一個簡單版本。
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Corruption(String),
    KeyNotFound,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {}", e),
            Error::Corruption(msg) => write!(f, "Data corruption: {}", msg),
            Error::KeyNotFound => write!(f, "Key not found"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
