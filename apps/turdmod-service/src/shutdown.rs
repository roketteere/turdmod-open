// Graceful shutdown signal — bridges the sync SCM stop handler to async tokio tasks.
// watch::Sender::send() is sync, so the SCM thread can fire it without a runtime.

use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone)]
pub struct Shutdown {
    tx: Arc<watch::Sender<bool>>,
}

impl Shutdown {
    pub fn new() -> (Self, watch::Receiver<bool>) {
        let (tx, rx) = watch::channel(false);
        (Self { tx: Arc::new(tx) }, rx)
    }

    pub fn trigger(&self) {
        let _ = self.tx.send(true);
    }
}

pub async fn wait(mut rx: watch::Receiver<bool>) {
    let _ = rx.wait_for(|v| *v).await;
}
