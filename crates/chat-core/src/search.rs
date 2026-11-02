//! Server search as a future: [`Client::search_hits`].
//!
//! The request goes over the client's connection like [`Client::search`]; the future
//! resolves with the hits (newest first) or a [`SearchError`]. It does not block: the wait
//! runs on the core runtime, and the returned future can be awaited from any executor (for
//! example the UI's). Dropping the future abandons the request.

use std::future::Future;
use std::time::Duration;

use crate::client::{Client, ClientError, ClientUpdate, ConnectionState, SearchRequest};
use crate::proto::{ErrorCode, SearchHit};

/// Extra time over the engine's request timeout before the future gives up by itself.
const GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchError {
    /// Bad input (empty or too long query, topic without group).
    #[error("invalid search: {0}")]
    Invalid(&'static str),
    /// The client is stopped, offline, or the connection dropped before the answer.
    #[error("not connected")]
    NotConnected,
    /// The server refused the search (for example `forbidden` for a group I left).
    #[error("search refused: {code:?}: {message}")]
    Refused { code: ErrorCode, message: String },
    /// No answer in time.
    #[error("search timed out")]
    Timeout,
}

impl Client {
    /// Searches on the server and returns the hits, newest first.
    ///
    /// Returns at once; await the future anywhere. Fails at once with `NotConnected` when
    /// the client is not connected.
    pub fn search_hits(
        &self,
        req: SearchRequest,
    ) -> impl Future<Output = Result<Vec<SearchHit>, SearchError>> + Send + 'static {
        let started = self.start_search(req);
        let deadline = self.config().request_timeout + GRACE;
        let task = started.map(|(rid, mut rx)| {
            self.spawn(async move {
                let wait = async {
                    while let Some(u) = rx.recv().await {
                        match u {
                            ClientUpdate::SearchResults { rid: r, hits } if r == rid => {
                                return Ok(hits);
                            }
                            ClientUpdate::SearchFailed {
                                rid: r,
                                code,
                                message,
                            } if r == rid => {
                                return Err(match code {
                                    Some(code) => SearchError::Refused { code, message },
                                    None => SearchError::NotConnected,
                                });
                            }
                            _ => {}
                        }
                    }
                    Err(SearchError::NotConnected)
                };
                tokio::time::timeout(deadline, wait)
                    .await
                    .unwrap_or(Err(SearchError::Timeout))
            })
        });
        async move {
            match task {
                Err(e) => Err(e),
                Ok(handle) => handle.await.unwrap_or(Err(SearchError::NotConnected)),
            }
        }
    }

    /// Subscribes before sending, so the answer cannot be missed.
    fn start_search(
        &self,
        req: SearchRequest,
    ) -> Result<(String, tokio::sync::mpsc::UnboundedReceiver<ClientUpdate>), SearchError> {
        if self.connection_state() != ConnectionState::Connected {
            // Validate first, so bad input reports as such even offline.
            return Err(validate(&req).err().unwrap_or(SearchError::NotConnected));
        }
        let rx = self.subscribe();
        let rid = self.search(req).map_err(|e| match e {
            ClientError::Invalid(m) => SearchError::Invalid(m),
            _ => SearchError::NotConnected,
        })?;
        Ok((rid, rx))
    }
}

fn validate(req: &SearchRequest) -> Result<(), SearchError> {
    let n = req.query.chars().count();
    if n == 0 || n > crate::proto::MAX_QUERY_CHARS {
        return Err(SearchError::Invalid("bad query"));
    }
    if req.topic_id.is_some() && req.group_id.is_none() {
        return Err(SearchError::Invalid("topic search needs a group"));
    }
    Ok(())
}
