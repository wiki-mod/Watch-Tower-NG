#![forbid(unsafe_code)]

//! ConvertibleNotifier and DelayNotifier traits.
//!
//! Translated from `old-source/pkg/types/convertible_notifier.go`.

use std::time::Duration;

/// A notifier capable of creating a shoutrrr URL.
pub trait ConvertibleNotifier {
    fn get_url(
        &self,
        command: &clap::Command,
    ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync + 'static>>;
}

/// A notifier that might need to be delayed before sending notifications.
pub trait DelayNotifier {
    fn get_delay(&self) -> Duration;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockConvertibleNotifier;

    impl ConvertibleNotifier for MockConvertibleNotifier {
        fn get_url(
            &self,
            _command: &clap::Command,
        ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync + 'static>>
        {
            Ok("slack://watchtower".to_string())
        }
    }

    struct MockDelayNotifier(Duration);

    impl DelayNotifier for MockDelayNotifier {
        fn get_delay(&self) -> Duration {
            self.0
        }
    }

    #[test]
    fn notifier_traits_preserve_legacy_contracts() {
        let notifier = MockConvertibleNotifier;
        let delay_notifier = MockDelayNotifier(Duration::from_secs(5));

        assert_eq!(
            notifier
                .get_url(&clap::Command::new("watchtower"))
                .expect("url should resolve"),
            "slack://watchtower"
        );
        assert_eq!(delay_notifier.get_delay(), Duration::from_secs(5));
    }
}
