use crate::domain::transfer::TransferStatus;

pub fn status_to_text(status: &TransferStatus) -> (&'static str, String) {
    match status {
        TransferStatus::Pending => ("pending", String::new()),
        TransferStatus::InProgress => ("progress", String::new()),
        TransferStatus::Completed => ("done", String::new()),
        TransferStatus::Failed(e) => ("failed", e.clone()),
    }
}
