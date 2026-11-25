//! Whether the client is in front of the player, where the system can send it
//! to the back and says so: Android. Everywhere else it is always in front.
//!
//! In the back, Android may keep the network from the client: its DNS then
//! answers nothing (`EAI_NODATA`) for as long as the client stays there, which
//! a battery or background restriction makes certain. A phone run showed chat
//! dropping and every retry failing so behind the game app, each one spent out
//! of the budget meant for an outage. So chat waits to reconnect until the
//! client is in front again (`infra::irc`, `services::reconnect`).

use std::sync::Arc;

use tokio::sync::watch;

/// The client's place on screen: in front until the shell says otherwise.
/// Cheap to clone, and every clone is the same place: the shell sets it, chat
/// reads it.
#[derive(Debug, Clone)]
pub struct Foreground(Arc<watch::Sender<bool>>);

impl Default for Foreground {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(true)))
    }
}

impl Foreground {
    /// Whether the client is in front now.
    pub fn in_front(&self) -> bool {
        *self.0.borrow()
    }

    /// The client is in front, or in the back.
    pub fn set(&self, in_front: bool) {
        self.0
            .send_if_modified(|now| std::mem::replace(now, in_front) != in_front);
    }

    /// Resolves once the client is in front: at once while it is.
    pub async fn front(&self) {
        // The sender is `self`'s, so it outlives the wait, which cannot fail.
        let _ = self.0.subscribe().wait_for(|in_front| *in_front).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn is_in_front_until_the_shell_says_otherwise() {
        let foreground = Foreground::default();
        assert!(foreground.in_front());
        let shared = foreground.clone();
        shared.set(false);
        assert!(!foreground.in_front());
        shared.set(true);
        assert!(foreground.in_front());
    }

    #[tokio::test(start_paused = true)]
    async fn the_front_is_waited_for_in_the_back_only() {
        let foreground = Foreground::default();
        // In front: no wait at all.
        tokio::time::timeout(Duration::ZERO, foreground.front())
            .await
            .expect("in front, the wait is over at once");

        foreground.set(false);
        let waiting = tokio::spawn({
            let foreground = foreground.clone();
            async move { foreground.front().await }
        });
        // However long the client stays in the back.
        tokio::time::sleep(Duration::from_secs(3600)).await;
        assert!(!waiting.is_finished());
        foreground.set(true);
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("back in front, the wait ends")
            .unwrap();
    }
}
