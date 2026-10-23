use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

pub(crate) struct Limiter {
    global: Arc<Semaphore>,
    per_host: Mutex<HashMap<String, Arc<Semaphore>>>,
    host_limit: usize,
}

pub(crate) struct LimitPermit {
    _global: OwnedSemaphorePermit,
    _host: OwnedSemaphorePermit,
}

impl Limiter {
    pub fn new(global_limit: usize, host_limit: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(global_limit.max(1))),
            per_host: Mutex::new(HashMap::new()),
            host_limit: host_limit.max(1),
        }
    }

    pub async fn acquire(
        &self,
        host: &str,
        cancel: &CancellationToken,
    ) -> Option<LimitPermit> {
        let host_semaphore = {
            let mut hosts = self.per_host.lock().ok()?;
            hosts
                .entry(host.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.host_limit)))
                .clone()
        };
        let host_permit = tokio::select! {
            permit = host_semaphore.acquire_owned() => permit.ok()?,
            _ = cancel.cancelled() => return None,
        };
        let global_permit = tokio::select! {
            permit = self.global.clone().acquire_owned() => permit.ok()?,
            _ = cancel.cancelled() => return None,
        };
        Some(LimitPermit {
            _global: global_permit,
            _host: host_permit,
        })
    }
}
