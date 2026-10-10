use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tokio::sync::broadcast;

use super::AccountTestEvent;

const EVENT_CAPACITY: usize = 64;
static SUBSCRIBERS: OnceLock<Mutex<HashMap<String, broadcast::Sender<AccountTestEvent>>>> =
    OnceLock::new();

/// Each test has a separate bounded queue, so another test cannot displace its
/// events. Dropping the HTTP body also drops its subscription immediately.
pub(crate) struct AccountTestAsyncEventSubscription {
    test_id: String,
    receiver: Option<broadcast::Receiver<AccountTestEvent>>,
}

impl AccountTestAsyncEventSubscription {
    pub(crate) async fn recv(&mut self) -> Result<AccountTestEvent, broadcast::error::RecvError> {
        match self.receiver.as_mut() {
            Some(receiver) => receiver.recv().await,
            None => Err(broadcast::error::RecvError::Closed),
        }
    }
}

impl Drop for AccountTestAsyncEventSubscription {
    fn drop(&mut self) {
        // Release this receiver before checking the sender's remaining count.
        self.receiver.take();
        if let Some(subscribers) = SUBSCRIBERS.get() {
            let mut guard =
                crate::lock_utils::lock_recover(subscribers, "account_test_async_subscribers");
            if guard
                .get(&self.test_id)
                .is_some_and(|sender| sender.receiver_count() == 0)
            {
                guard.remove(&self.test_id);
            }
        }
    }
}

pub(crate) fn subscribe_account_test_events_async(
    test_id: &str,
) -> AccountTestAsyncEventSubscription {
    let subscribers = SUBSCRIBERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = crate::lock_utils::lock_recover(subscribers, "account_test_async_subscribers");
    let receiver = guard
        .entry(test_id.to_owned())
        .or_insert_with(|| broadcast::channel(EVENT_CAPACITY).0)
        .subscribe();
    AccountTestAsyncEventSubscription {
        test_id: test_id.to_owned(),
        receiver: Some(receiver),
    }
}

pub(super) fn publish_account_test_event(event: AccountTestEvent) {
    if let Some(subscribers) = SUBSCRIBERS.get() {
        let guard = crate::lock_utils::lock_recover(subscribers, "account_test_async_subscribers");
        if let Some(sender) = guard.get(&event.test_id) {
            let _ = sender.send(event);
        }
    }
}

#[cfg(test)]
pub(crate) fn account_test_async_subscriber_count(test_id: &str) -> usize {
    SUBSCRIBERS.get().map_or(0, |subscribers| {
        crate::lock_utils::lock_recover(subscribers, "account_test_async_subscribers")
            .get(test_id)
            .map_or(0, broadcast::Sender::receiver_count)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn async_account_subscriptions_preserve_test_isolation_and_legacy_delivery() {
        let first_id = "async-subscription-first";
        let second_id = "async-subscription-second";
        let first = subscribe_account_test_events_async(first_id);
        let second = subscribe_account_test_events_async(second_id);
        let legacy = super::super::subscribe_account_test_events(first_id);

        super::super::notify_account_test_event(AccountTestEvent::new(first_id, "delta"));

        let mut first = first;
        assert_eq!(
            first.receiver.as_mut().unwrap().try_recv().unwrap().test_id,
            first_id
        );
        let mut second = second;
        assert!(matches!(
            second.receiver.as_mut().unwrap().try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(
            legacy
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .test_id,
            first_id
        );
    }

    #[test]
    fn dropping_last_async_account_subscriber_removes_test_channel() {
        let test_id = "async-subscription-drop";
        let first = subscribe_account_test_events_async(test_id);
        let second = subscribe_account_test_events_async(test_id);
        drop(first);
        assert!(SUBSCRIBERS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .contains_key(test_id));
        drop(second);
        assert!(!SUBSCRIBERS
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .contains_key(test_id));
    }

    #[test]
    fn cancelled_receive_keeps_event_and_other_tests_cannot_overflow_subscription() {
        use futures_util::FutureExt;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let test_id = "async-subscription-cancel";
            let other_id = "async-subscription-busy";
            let mut receiver = subscribe_account_test_events_async(test_id);
            let _other = subscribe_account_test_events_async(other_id);
            assert!(receiver.recv().now_or_never().is_none());
            publish_account_test_event(AccountTestEvent::new(test_id, "done"));
            for _ in 0..EVENT_CAPACITY * 2 {
                publish_account_test_event(AccountTestEvent::new(other_id, "delta"));
            }
            let event = receiver
                .recv()
                .await
                .expect("event after cancelled receive");
            assert_eq!(event.test_id, test_id);
            assert_eq!(event.event_type, "done");
        });
    }
}
