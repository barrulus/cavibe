use anyhow::Result;
use mpris::PlayerFinder;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::interval;
use tracing::{debug, warn};

/// Current track metadata
#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
}

/// MPRIS metadata watcher
pub struct MetadataWatcher {
    sender: watch::Sender<Arc<TrackInfo>>,
}

impl MetadataWatcher {
    pub fn new() -> (Self, watch::Receiver<Arc<TrackInfo>>) {
        let (sender, receiver) = watch::channel(Arc::new(TrackInfo::default()));
        (Self { sender }, receiver)
    }

    pub async fn run(self) -> Result<()> {
        let mut poll_interval = interval(Duration::from_secs(1));

        loop {
            poll_interval.tick().await;

            let track_info = match Self::fetch_current_track() {
                Ok(info) => info,
                Err(e) => {
                    debug!("Failed to fetch track info: {}", e);
                    TrackInfo::default()
                }
            };

            let _ = self.sender.send(Arc::new(track_info));
        }
    }

    fn fetch_current_track() -> Result<TrackInfo> {
        let finder = PlayerFinder::new()?;

        // Find active player
        let player = finder.find_active().or_else(|_| {
            finder
                .find_all()
                .map_err(|_| mpris::DBusError::Miscellaneous("Failed to find players".into()))?
                .into_iter()
                .next()
                .ok_or_else(|| mpris::DBusError::Miscellaneous("No players found".into()))
        })?;

        let metadata = player.get_metadata()?;

        Ok(TrackInfo {
            title: metadata.title().map(|s| s.to_string()),
            artist: metadata.artists().map(|a| a.join(", ")),
        })
    }
}

/// Start the metadata watcher in the background
pub fn start_watcher() -> watch::Receiver<Arc<TrackInfo>> {
    let (watcher, receiver) = MetadataWatcher::new();

    tokio::spawn(async move {
        if let Err(e) = watcher.run().await {
            warn!("Metadata watcher error: {}", e);
        }
    });

    receiver
}
