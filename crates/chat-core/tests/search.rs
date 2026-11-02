//! `Client::search_hits` against the mock server.

mod common;
mod mock;

use chat_core::proto::ErrorCode;
use chat_core::{Client, ClientConfig, ConnectionState, SearchError, SearchRequest};
use common::*;
use mock::{MockServer, TOKEN};
use std::time::{Duration, Instant};

async fn connected() -> (MockServer, tempfile::TempDir, Client) {
    let server = MockServer::start(
        member(ME, "me"),
        vec![member(ME, "me"), member(ALICE, "alice")],
    )
    .await;
    server.add_group(group(G1, &[ME, ALICE]));
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = ClientConfig::new(server.base_url(), TOKEN, dir.path());
    cfg.request_timeout = Duration::from_secs(5);
    let client = Client::new(cfg).unwrap();
    client.start();
    let t0 = Instant::now();
    while client.connection_state() != ConnectionState::Connected {
        assert!(t0.elapsed() < Duration::from_secs(10), "not connected");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    (server, dir, client)
}

fn req(query: &str) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn returns_hits_for_each_request() {
    let (server, _dir, client) = connected().await;
    // Two searches at once: each future gets the answer to its own request.
    let a = client.search_hits(req("部署"));
    let b = client.search_hits(SearchRequest {
        query: "prod".into(),
        group_id: Some(G1.into()),
        limit: Some(5),
        ..Default::default()
    });
    let (a, b) = tokio::join!(a, b);
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].snippet, "部署");
    assert_eq!(b[0].snippet, "prod");
    let frames = server.with(|s| s.frames_of("search"));
    assert_eq!(frames.len(), 2);
    let scoped = frames.iter().find(|f| f["query"] == "prod").unwrap();
    assert_eq!(
        (scoped["groupId"].as_str(), scoped["limit"].as_u64()),
        (Some(G1), Some(5))
    );
}

#[tokio::test]
async fn reports_server_refusal() {
    let (server, _dir, client) = connected().await;
    server.with(|s| s.search_error = Some(ErrorCode::Forbidden));
    let err = client.search_hits(req("x")).await.unwrap_err();
    assert!(
        matches!(
            err,
            SearchError::Refused {
                code: ErrorCode::Forbidden,
                ..
            }
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn fails_at_once_when_offline_or_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new("http://127.0.0.1:9", TOKEN, dir.path())).unwrap();
    let t0 = Instant::now();
    assert_eq!(
        client.search_hits(req("部署")).await.unwrap_err(),
        SearchError::NotConnected
    );
    assert_eq!(
        client.search_hits(req("")).await.unwrap_err(),
        SearchError::Invalid("bad query")
    );
    assert!(matches!(
        client
            .search_hits(SearchRequest {
                query: "a".into(),
                topic_id: Some("T".into()),
                ..Default::default()
            })
            .await,
        Err(SearchError::Invalid(_))
    ));
    assert!(t0.elapsed() < Duration::from_secs(1));
}

#[test]
fn can_be_awaited_outside_tokio() {
    // The UI executor is not tokio: the future must still complete there.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (server, dir, client) = rt.block_on(connected());
    let fut = client.search_hits(req("hello"));
    let hits = std::thread::spawn(move || futures_lite_block_on(fut))
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(hits[0].snippet, "hello");
    drop((server, dir, client));
    drop(rt);
}

/// A minimal executor with no runtime: parks the thread until woken.
fn futures_lite_block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}
