//! 无需硬件的 mock 设备。

use std::sync::{Arc, Mutex};

use agent_beacon_core::Effect;

use crate::transport::Transport;
use crate::{hex, CommandPlan, PlannedCommand};

/// mock 设备收到的命令序列，供开发与测试断言。
#[derive(Debug, Clone, Default)]
pub struct MockLog {
    entries: Arc<Mutex<Vec<String>>>,
}

impl MockLog {
    pub fn record(&self, bytes: &[u8]) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(hex(bytes));
            if entries.len() > 50 {
                entries.remove(0);
            }
        }
    }

    pub fn entries(&self) -> Vec<String> {
        self.entries
            .lock()
            .map(|entries| entries.clone())
            .unwrap_or_default()
    }
}

pub struct MockTransport {
    log: MockLog,
}

impl MockTransport {
    pub fn new(log: MockLog) -> Self {
        Self { log }
    }
}

impl Transport for MockTransport {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.log.record(bytes);
        Ok(())
    }

    fn describe(&self) -> String {
        "mock".to_string()
    }
}

pub fn plan(effect: Effect) -> CommandPlan {
    let label = format!("MOCK:{}", effect.as_str());
    CommandPlan {
        dedupe_key: format!("mock:{}", effect.as_str()),
        commands: vec![PlannedCommand {
            delay_ms: 0,
            bytes: label.clone().into_bytes(),
            label,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_records_commands() {
        let log = MockLog::default();
        let mut transport = MockTransport::new(log.clone());
        transport.write(b"OFF\n").unwrap();
        assert_eq!(log.entries(), vec!["4F 46 46 0A".to_string()]);
    }
}
