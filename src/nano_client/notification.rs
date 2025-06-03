use crate::packetview::QoS;
use crate::transport_client::Notification;

#[derive(Debug)]
pub struct TransportNotification<'a> {
    /// The underlying notification received from the transport client
    pub notification: Notification<'a>,
    /// The amount of data this notification takes in the RX buffer
    pub(super) taken: usize,
}

/// The action to take when completing a notification
#[derive(Debug)]
pub enum NotificationAction {
    None,
    PingReq,
    PubAck(u16),
    PubRec(u16),
    PubComp(u16),
}

/// Each notification should be completed to ensure buffers are handled correctly
pub struct NotificationCompletion {
    pub(super) taken: usize,
    pub(super) action: NotificationAction,
}

impl NotificationCompletion {
    /// Create a `NotificationCompletion` with a specific action.
    pub fn with_action(tick_result: ClientNotification, action: NotificationAction) -> Self {
        Self {
            taken: tick_result.taken(),
            action,
        }
    }
}

/// A notification returned from [`NanoClient::next_notification()`]
#[derive(Debug)]
pub enum ClientNotification<'a> {
    /// The transport has completed a notification.
    TransportNotification(TransportNotification<'a>),
    /// The client wants the user to send a ping.
    SendPing,
}

impl ClientNotification<'_> {
    fn taken(&self) -> usize {
        match self {
            ClientNotification::TransportNotification(x) => x.taken,
            ClientNotification::SendPing => 0,
        }
    }
    /// Complete a [`ClientNotification`] with a default completion token.
    ///
    /// The following mappings of notification are used:
    ///
    /// | Notification  | Action         |
    /// |---------------|----------------|
    /// | SendPing      | Send `PingReq` |
    /// | Publish QoS 0 | No action      |
    /// | Publish QoS 1 | Send `PubAck`  |
    /// | Publish QoS 2 | Send `PubRec`  |
    /// | PubRel        | Send `PubComp` |
    /// | All other     | No action      |
    ///
    pub fn complete(self) -> NotificationCompletion {
        match self {
            Self::SendPing => NotificationCompletion {
                taken: 0,
                action: NotificationAction::PingReq,
            },
            Self::TransportNotification(x) => {
                let taken = x.taken;
                let action = match x.notification {
                    Notification::Publish(publish) => match publish.qos {
                        QoS::AtMostOnce => NotificationAction::None,
                        QoS::AtLeastOnce => NotificationAction::PubAck(publish.pkid),
                        QoS::ExactlyOnce => NotificationAction::PubRec(publish.pkid),
                    },
                    Notification::PubRel(pubrel) => NotificationAction::PubComp(pubrel.pkid),
                    _ => NotificationAction::None,
                };

                NotificationCompletion { taken, action }
            }
        }
    }

    /// Complete a [`ClientNotification`] with no action.
    ///
    pub fn complete_no_action(self) -> NotificationCompletion {
        NotificationCompletion {
            taken: self.taken(),
            action: NotificationAction::None,
        }
    }

    /// Complete a [`ClientNotification`] with a default completion token.
    ///
    /// The following mappings of notification are used:
    ///
    /// | Notification  | Action         |
    /// |---------------|----------------|
    /// | SendPing      | Send `PingReq` |
    /// | All other     | No action      |
    ///
    pub fn complete_no_pub_ack(self) -> NotificationCompletion {
        match self {
            ClientNotification::TransportNotification(x) => NotificationCompletion {
                taken: x.taken,
                action: NotificationAction::None,
            },
            ClientNotification::SendPing => NotificationCompletion {
                taken: 0,
                action: NotificationAction::PingReq,
            },
        }
    }
}
