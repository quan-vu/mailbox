//! Run a short job on the account's held connection when the live worker has one, otherwise on a
//! connection of its own (MB-151 Sent copy, MB-133 server drafts; MB-140 live session).
//!
//! Only jobs that are safe to run a second time belong here ([`RepeatableJob`]): when the borrowed
//! connection breaks during the job, it is dropped (the worker reconnects) and the job runs once
//! more on a fresh connection. A job that writes must therefore check first (the Sent copy looks
//! for its `Message-ID` before it APPENDs), so a write that reached the server before the break is
//! not made twice.
use crate::Session;
use crate::append::AppendError;
use crate::live::LiveHandle;
use std::future::Future;
use tokio::io::{AsyncRead, AsyncWrite};

/// A short IMAP job that may run twice without harm.
pub trait RepeatableJob {
    type Output;
    fn run<'a, S: AsyncRead + AsyncWrite + Unpin + Send + 'a>(
        &'a self,
        session: &'a mut Session<S>,
    ) -> impl Future<Output = Result<Self::Output, AppendError>> + Send + 'a;
}

/// Which connection did the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The live worker's held session.
    Live,
    /// A connection opened for this job (no worker, or it was not connected).
    Fresh,
    /// The held session broke during the job; it was dropped and the job ran again on a fresh one.
    LiveThenFresh,
}

/// Run `job` on the held session of `live` if it is connected, else on `fresh()`. A fresh
/// connection is logged out afterwards; the held one goes back to the worker.
pub async fn live_or_fresh<L, F, J, C, Fut>(
    live: Option<&LiveHandle<L>>,
    job: &J,
    fresh: C,
) -> (Result<J::Output, AppendError>, Route)
where
    L: AsyncRead + AsyncWrite + Unpin + Send,
    F: AsyncRead + AsyncWrite + Unpin + Send,
    J: RepeatableJob,
    C: FnOnce() -> Fut,
    Fut: Future<Output = Result<Session<F>, AppendError>>,
{
    let mut route = Route::Fresh;
    if let Some(handle) = live
        && let Some(mut borrowed) = handle.session().await
    {
        match job.run(borrowed.reader_mut()).await {
            Err(AppendError::Connection(_)) => {
                borrowed.discard();
                route = Route::LiveThenFresh;
            }
            other => return (other, Route::Live),
        }
    }
    let mut session = match fresh().await {
        Ok(s) => s,
        Err(e) => return (Err(e), route),
    };
    let result = job.run(&mut session).await;
    session.logout().await;
    (result, route)
}
