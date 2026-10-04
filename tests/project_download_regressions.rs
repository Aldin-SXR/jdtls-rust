//! Local HTTP coverage of repository fallback, caching and concurrent Maven writes.
#[path = "../src/project/download.rs"]
#[allow(dead_code)]
mod download;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

fn repository(
    responses: Vec<(u16, Vec<u8>)>,
) -> (download::Repository, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let repo = download::Repository {
        id: "local-test".into(),
        url: format!("http://{}", listener.local_addr().unwrap()),
    };
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("missing download request: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|s| s == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
            }
            requests.push(
                String::from_utf8(request)
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned(),
            );
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
        requests
    });
    (repo, handle)
}

#[test]
fn falls_back_after_404_and_reuses_the_cached_artifact() {
    let local = tempfile::tempdir().unwrap();
    let (missing, first) = repository(vec![(404, b"missing".to_vec())]);
    let payload = b"a complete artifact\0with binary bytes".to_vec();
    let (found, second) = repository(vec![(200, payload.clone())]);
    let repos = [missing, found];
    let path = download::fetch(
        local.path(),
        &repos,
        "org.example",
        "sample",
        "1.0",
        None,
        "jar",
    )
    .unwrap();
    assert_eq!(payload, std::fs::read(&path).unwrap());
    assert_eq!(
        Some(path.clone()),
        download::fetch(
            local.path(),
            &repos,
            "org.example",
            "sample",
            "1.0",
            None,
            "jar"
        )
    );
    let expected = "GET /org/example/sample/1.0/sample-1.0.jar HTTP/1.1";
    assert_eq!(vec![expected], first.join().unwrap());
    assert_eq!(vec![expected], second.join().unwrap());
    assert!(
        std::fs::read_to_string(path.parent().unwrap().join("_remote.repositories"))
            .unwrap()
            .contains("sample-1.0.jar>local-test=")
    );
}

#[test]
fn concurrent_artifacts_preserve_complete_files_and_both_origins() {
    let local = tempfile::tempdir().unwrap();
    let payload = vec![42; 64 * 1024];
    let (repo, server) = repository(vec![(200, payload.clone()), (200, payload.clone())]);
    let paths = thread::scope(|scope| {
        let one = scope.spawn(|| {
            download::fetch(
                local.path(),
                &[repo.clone()],
                "org.example",
                "sample",
                "1.0",
                None,
                "jar",
            )
            .unwrap()
        });
        let two = scope.spawn(|| {
            download::fetch(
                local.path(),
                &[repo.clone()],
                "org.example",
                "sample",
                "1.0",
                Some("sources"),
                "jar",
            )
            .unwrap()
        });
        [one.join().unwrap(), two.join().unwrap()]
    });
    assert_eq!(2, server.join().unwrap().len());
    for path in &paths {
        assert_eq!(payload, std::fs::read(path).unwrap());
    }
    let dir = paths[0].parent().unwrap();
    let origins = std::fs::read_to_string(dir.join("_remote.repositories")).unwrap();
    assert!(origins.contains("sample-1.0.jar>local-test="));
    assert!(origins.contains("sample-1.0-sources.jar>local-test="));
    assert_eq!(3, std::fs::read_dir(dir).unwrap().count());
}
