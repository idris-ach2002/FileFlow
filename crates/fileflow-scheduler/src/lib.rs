use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBudget {
    pub cpu_tokens: usize,
    pub memory_mb: u64,
    pub io_tokens: usize,
}

impl ResourceBudget {
    pub fn balanced() -> Self {
        let logical = std::thread::available_parallelism().map_or(2, usize::from);
        let reserve = usize::from(logical >= 4) + usize::from(logical >= 8);

        Self {
            cpu_tokens: logical.saturating_sub(reserve).max(1),
            memory_mb: 4 * 1024,
            io_tokens: 4,
        }
    }
}
