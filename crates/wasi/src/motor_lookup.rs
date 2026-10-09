//! A single outstanding parent-ID lookup using Motor's existing CMD_STAT.
use moto_io::fs::{EntryId, EntryKind};
use moto_ipc::io_channel::{Receiver, Sender};
use moto_sys_io::api_fs;
use std::cell::RefCell;

struct Connection {
    sender: Sender,
    receiver: Receiver,
    sequence: u64,
}

thread_local! {
    static IDLE: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

pub(super) async fn lookup(parent: EntryId, name: &str) -> moto_rt::Result<(EntryId, EntryKind)> {
    if name.is_empty()
        || name.len() > moto_rt::fs::MAX_FILENAME_LEN
        || name.contains(['/', '\0'])
        || matches!(name, "." | "..")
    {
        return Err(moto_rt::Error::InvalidFilename);
    }
    // The future owns the lease. Cancellation or transport failure closes it,
    // reclaiming donated pages and preventing a stale response from reuse.
    let idle = IDLE.with(|slot| slot.borrow_mut().take());
    let mut connection = match idle {
        Some(connection) => connection,
        None => {
            let (sender, receiver) = moto_ipc::io_channel::connect(api_fs::FS_URL)?;
            Connection {
                sender,
                receiver,
                sequence: 0,
            }
        }
    };
    connection.sequence = connection
        .sequence
        .checked_add(1)
        .ok_or(moto_rt::Error::InternalError)?;
    let page = connection.sender.alloc_page(u64::MAX).await?;
    let mut request = api_fs::stat_msg_encode(parent, name, page);
    request.id = connection.sequence;
    connection.sender.send(request).await?;
    let response = connection.receiver.recv().await?;
    if response.id != request.id {
        return Err(moto_rt::Error::InternalError);
    }
    // sys-io answers a failed request with its status alone, without a command.
    let result = match response.status() {
        Err(error) => Err(error),
        Ok(()) if response.command != api_fs::CMD_STAT => {
            return Err(moto_rt::Error::InternalError);
        }
        Ok(()) => api_fs::stat_resp_decode(response),
    };
    IDLE.with(|slot| {
        // A reentrant lookup can have returned its own lease in the meantime.
        // Keep at most one idle channel rather than multiplexing responses.
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(connection);
        }
    });
    result
}
