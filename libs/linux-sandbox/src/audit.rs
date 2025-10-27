use serde_json;
use std::path::PathBuf;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone, Debug, serde::Serialize)]
pub enum AuditEvent {
    FileAccess {
        action: String,
        path: String,
        allowed: bool,
    },
    NetworkAccess {
        action: String,
        resource: String,
        allowed: bool,
    },
    CommandExecution {
        command: String,
        allowed: bool,
        destructive: bool,
    },
    SecurityBlock {
        reason: String,
        details: String,
    },
}

pub struct AuditLogger {
    enabled: bool,
    log_file: Option<PathBuf>,
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl AuditLogger {
    pub fn new(enabled: bool, log_file: Option<String>) -> Self {
        let log_file = log_file.and_then(|f| expand_path(&f));
        
        Self {
            enabled,
            log_file,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
    
    pub fn log_event(&self, event: AuditEvent) {
        if !self.enabled {
            return;
        }
        
        log::debug!("Audit: {:?}", event);
        
        // Store in memory
        if let Ok(mut events) = self.events.lock() {
            events.push(event.clone());
        }
        
        // Write to file if configured
        if let Some(ref log_file) = self.log_file {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file)
            {
                if let Ok(json) = serde_json::to_string_pretty(&event) {
                    let _ = writeln!(file, "{}", json);
                }
            }
        }
    }
    
    pub fn log_file_access(&self, action: &str, path: &str, allowed: bool) {
        self.log_event(AuditEvent::FileAccess {
            action: action.to_string(),
            path: path.to_string(),
            allowed,
        });
    }
    
    pub fn log_network(&self, action: &str, resource: &str, allowed: bool) {
        self.log_event(AuditEvent::NetworkAccess {
            action: action.to_string(),
            resource: resource.to_string(),
            allowed,
        });
    }
    
    pub fn log_command(&self, command: &str, allowed: bool, destructive: bool) {
        self.log_event(AuditEvent::CommandExecution {
            command: command.to_string(),
            allowed,
            destructive,
        });
    }
    
    pub fn log_security_block(&self, reason: &str, details: &str) {
        self.log_event(AuditEvent::SecurityBlock {
            reason: reason.to_string(),
            details: details.to_string(),
        });
    }
    
    pub fn get_events(&self) -> Vec<AuditEvent> {
        self.events.lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

fn expand_path(path: &str) -> Option<PathBuf> {
    if path.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join(&path[2..]));
        }
    }
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_audit_logger_creation() {
        let logger = AuditLogger::new(true, Some("test.log".to_string()));
        assert!(logger.enabled);
        
        let logger2 = AuditLogger::new(false, None);
        assert!(!logger2.enabled);
    }
    
    #[test]
    fn test_log_file_access() {
        let logger = AuditLogger::new(true, None);
        logger.log_file_access("read", "/etc/passwd", false);
        logger.log_file_access("write", "/tmp/test", true);
        
        let events = logger.get_events();
        assert_eq!(events.len(), 2);
    }
}

