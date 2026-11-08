//! Volatile transfer-rate samples. No credentials or progress are persisted here.
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
const WINDOW: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub(crate) struct Rates(Arc<Mutex<HashMap<String, Sample>>>);
struct Sample {
    baseline: Instant,
    points: VecDeque<(Instant, u64)>,
    revision: u64,
}
impl Default for Rates {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }
}
impl Rates {
    pub fn start(
        &self,
        id: &str,
        revision: u64,
        committed: u64,
    ) -> nekopak_provider::DownloadObserver {
        let now = Instant::now();
        if let Ok(mut map) = self.0.lock() {
            let sample = map.entry(id.into()).or_insert_with(|| Sample {
                baseline: now,
                points: VecDeque::from([(now, committed)]),
                revision,
            });
            sample.revision = revision;
        }
        let rates = self.clone();
        let id = id.to_owned();
        nekopak_provider::DownloadObserver::new(move |bytes| {
            if let Ok(mut map) = rates.0.lock() {
                if let Some(sample) = map.get_mut(&id).filter(|s| s.revision == revision) {
                    sample.record(Instant::now(), committed.saturating_add(bytes));
                }
            }
        })
    }
    pub fn speed(&self, id: &str) -> Option<f64> {
        self.0.lock().ok()?.get_mut(id)?.speed(Instant::now())
    }
    pub fn remove(&self, id: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(id);
        }
    }
}
impl Sample {
    fn record(&mut self, now: Instant, bytes: u64) {
        if self.points.back().is_some_and(|(_, n)| bytes < *n) {
            return;
        }
        // Bound memory independently of network frame size, preserving the baseline.
        if self.points.len() > 1
            && self
                .points
                .back()
                .is_some_and(|(t, _)| now.duration_since(*t) < Duration::from_millis(100))
        {
            self.points.pop_back();
        }
        self.points.push_back((now, bytes));
        self.trim(now);
    }
    fn trim(&mut self, now: Instant) {
        while self.points.len() > 2 && now.duration_since(self.points[1].0) >= WINDOW {
            self.points.pop_front();
        }
    }
    fn speed(&mut self, now: Instant) -> Option<f64> {
        self.trim(now);
        let (_, last) = *self.points.back()?;
        if last == 0 {
            return Some(0.0);
        }
        let (at, first) = *self.points.front()?;
        if now.duration_since(self.points.back()?.0) >= WINDOW {
            return Some(0.0);
        }
        let elapsed = now.duration_since(at.max(self.baseline)).as_secs_f64();
        (elapsed > 0.0).then_some((last - first) as f64 / elapsed)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_follow_bytes_and_time_then_decay_after_network_stalls() {
        let start = Instant::now();
        let mut sample = Sample {
            baseline: start,
            points: VecDeque::from([(start, 0)]),
            revision: 1,
        };
        sample.record(start + Duration::from_secs(1), 1024);
        assert_eq!(sample.speed(start + Duration::from_secs(1)), Some(1024.0));
        assert_eq!(sample.speed(start + Duration::from_secs(2)), Some(512.0));
        assert_eq!(sample.speed(start + Duration::from_secs(4)), Some(0.0));
    }
}
