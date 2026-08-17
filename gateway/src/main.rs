//! The private-mode Pingora gateway executable.

use gateway::proxy::PrivateGateway;
use gateway::GatewayConfig;
use pingora::proxy::http_proxy_service;
use pingora::server::Server;

fn main() {
    dotenvy::dotenv().ok();
    let config = GatewayConfig::from_env().unwrap_or_else(|error| {
        eprintln!("gateway configuration error: {error}");
        std::process::exit(2);
    });
    let listen = config.listen.clone();
    let mut server = Server::new(None).expect("Pingora server bootstrap");
    server.bootstrap();
    let mut service = http_proxy_service(&server.configuration, PrivateGateway::new(config));
    service.add_tcp(&listen);
    server.add_service(service);
    server.run_forever();
}
