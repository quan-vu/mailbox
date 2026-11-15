//! Folder listing with attributes for role detection (MB-122). Read-only (LIST).
use crate::{Session, parse};
use mailbox_core::MailboxError;
use mailbox_core::folders::{FolderSource, ListedFolder};
use tokio::io::{AsyncRead, AsyncWrite};

impl<S: AsyncRead + AsyncWrite + Unpin> FolderSource for Session<S> {
    async fn list_detailed(&mut self) -> Result<Vec<ListedFolder>, MailboxError> {
        Ok(Session::list_folders(self)
            .await?
            .into_iter()
            .map(|f| ListedFolder {
                selectable: f.selectable(),
                display_name: parse::decode_mutf7(&f.name),
                name: f.name,
                delimiter: f.delimiter,
                attributes: f.flags,
            })
            .collect())
    }
}
