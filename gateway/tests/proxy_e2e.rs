//! A live Pingora gateway in front of a small HTTP upstream.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use gateway::proxy::PrivateGateway;
use gateway::GatewayConfig;
use pingora::proxy::http_proxy_service;
use pingora::server::Server;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn upstream() -> (u16, mpsc::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut lines = Vec::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                lines.push(line.trim_end().to_owned());
            }
            sender.send(lines).unwrap();
            let mut stream = stream;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .unwrap();
        }
    });
    (port, receiver)
}

fn config(listen: u16, upstream: u16, nets: &str) -> GatewayConfig {
    GatewayConfig::from_getter(|name| match name {
        "GATEWAY_LISTEN" => Some(format!("127.0.0.1:{listen}")),
        "GATEWAY_UPSTREAM" => Some(format!("127.0.0.1:{upstream}")),
        "GATEWAY_ALLOWED_NETS" => Some(nets.into()),
        "GATEWAY_BASIC_AUTH" => Some("go:bears".into()),
        _ => None,
    })
    .unwrap()
}

fn wait_for_port(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "gateway did not bind {port}");
        thread::sleep(Duration::from_millis(25));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_live_gateway_challenges_forbids_and_proxies() {
    let (upstream_port, requests) = upstream();
    let open_port = free_port();
    let denied_port = free_port();
    thread::spawn(move || {
        let mut server = Server::new(None).unwrap();
        server.bootstrap();
        let mut allowed = http_proxy_service(
            &server.configuration,
            PrivateGateway::new(config(open_port, upstream_port, "127.0.0.0/8")),
        );
        allowed.add_tcp(&format!("127.0.0.1:{open_port}"));
        server.add_service(allowed);
        let mut denied = http_proxy_service(
            &server.configuration,
            PrivateGateway::new(config(denied_port, upstream_port, "10.0.0.0/8")),
        );
        denied.add_tcp(&format!("127.0.0.1:{denied_port}"));
        server.add_service(denied);
        server.run_forever();
    });
    wait_for_port(open_port);
    wait_for_port(denied_port);
    let client = reqwest::Client::new();
    let open = |path: &str| format!("http://127.0.0.1:{open_port}{path}");

    let response = client.get(open("/")).send().await.unwrap();
    assert_eq!(response.status(), 401);
    assert!(response.headers()["www-authenticate"]
        .to_str()
        .unwrap()
        .contains("Basic"));

    let response = client.get(open("/health")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ok");
    assert!(requests.recv_timeout(Duration::from_secs(3)).unwrap()[0].starts_with("GET /health"));

    let response = client
        .get(open("/app/team"))
        .basic_auth("go", Some("bears"))
        .header("x-forwarded-for", "203.0.113.7")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let headers = requests.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(headers
        .iter()
        .any(|line| line.eq_ignore_ascii_case("x-forwarded-for: 203.0.113.7, 127.0.0.1")));

    let response = client
        .get(format!("http://127.0.0.1:{denied_port}/app/team"))
        .basic_auth("go", Some("bears"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    assert!(requests.recv_timeout(Duration::from_millis(300)).is_err());
}
