use std::collections::VecDeque;
use tokio::sync::Mutex;

pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
    max_entries: usize,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub deployment_id: String,
    pub body: String,
    pub severity: String,
    pub resource_name: Option<String>,
    pub attributes: Vec<(String, String)>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_entries: 10000,
        }
    }

    pub async fn push(&self, entry: LogEntry) {
        let mut entries = self.entries.lock().await;
        if entries.len() >= self.max_entries {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub async fn get_entries(&self, deployment_id: Option<&str>, limit: usize) -> Vec<LogEntry> {
        let entries = self.entries.lock().await;
        entries
            .iter()
            .filter(|e| deployment_id.map_or(true, |id| e.deployment_id == id))
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}
