pub enum PublishItem {
    Unused,
    WaitingPubAck(u16),
}

pub struct PublishState {}
