use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_EVENTS: usize = 200;

#[derive(Debug, Clone)]
pub enum EventKind {
    Request,
    CssReload,
    FullReload,
    /// Bundle / push operations logged from background tasks or TUI hotkeys
    Build,
}

#[derive(Debug, Clone)]
pub struct EventEntry {
    /// HH:MM:SS
    pub time: String,
    pub kind: EventKind,
    /// Short label shown in the label column, e.g. "GET", "CSS", "RELOAD"
    pub label: String,
    pub message: String,
    /// Extra info shown at the right, e.g. "200  12ms"
    pub extra: String,
}

pub struct ServerStats {
    pub started_at: Instant,
    pub requests_total: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub css_compilations: u64,
    pub live_reloads: u64,
    pub css_reloads: u64,
    pub total_response_ms: u64,
    pub recent_events: VecDeque<EventEntry>,
}

impl ServerStats {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            requests_total: 0,
            cache_hits: 0,
            cache_misses: 0,
            css_compilations: 0,
            live_reloads: 0,
            css_reloads: 0,
            total_response_ms: 0,
            recent_events: VecDeque::with_capacity(MAX_EVENTS),
        }
    }

    pub fn record_request(&mut self, method: &str, path: &str, status: u16, duration: Duration) {
        self.requests_total += 1;
        self.total_response_ms += duration.as_millis() as u64;
        self.push_event(EventEntry {
            time: current_time_str(),
            kind: EventKind::Request,
            label: method.to_string(),
            message: truncate_path(path, 45),
            extra: format!("{}  {}ms", status, duration.as_millis()),
        });
    }

    pub fn record_full_reload(&mut self, path: &str) {
        self.live_reloads += 1;
        self.push_event(EventEntry {
            time: current_time_str(),
            kind: EventKind::FullReload,
            label: "RELOAD".to_string(),
            message: truncate_path(path, 55),
            extra: String::new(),
        });
    }

    pub fn record_css_reload(&mut self, path: &str) {
        self.css_reloads += 1;
        self.push_event(EventEntry {
            time: current_time_str(),
            kind: EventKind::CssReload,
            label: "CSS".to_string(),
            message: truncate_path(path, 55),
            extra: String::new(),
        });
    }

    pub fn record_build_event(&mut self, message: &str, extra: &str) {
        self.push_event(EventEntry {
            time: current_time_str(),
            kind: EventKind::Build,
            label: "BUILD".to_string(),
            message: message.to_string(),
            extra: extra.to_string(),
        });
    }

    pub fn avg_response_ms(&self) -> u64 {
        if self.requests_total == 0 {
            0
        } else {
            self.total_response_ms / self.requests_total
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64 * 100.0
        }
    }

    pub fn uptime_str(&self) -> String {
        let secs = self.started_at.elapsed().as_secs();
        format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    }

    fn push_event(&mut self, event: EventEntry) {
        if self.recent_events.len() >= MAX_EVENTS {
            self.recent_events.pop_front();
        }
        self.recent_events.push_back(event);
    }
}

impl Default for ServerStats {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedStats = Arc<Mutex<ServerStats>>;

fn current_time_str() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = now % 86400;
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn truncate_path(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("…{}", &s[s.len().saturating_sub(max - 1)..])
    }
}
