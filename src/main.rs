use clap::Parser;
use std::error::Error;
use std::fmt::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, instrument, warn};

// SOCKS Protocol Constants
const SOCKS5_VERSION: u8 = 0x05;
const SOCKS5_AUTH_NONE: u8 = 0x00;
const SOCKS5_AUTH_METHODS: u8 = 0x01;
const SOCKS5_CMD_CONNECT: u8 = 0x01;
const SOCKS5_RSV: u8 = 0x00;
const SOCKS5_ATYP_IPV4: u8 = 0x01;
const SOCKS5_ATYP_DOMAIN: u8 = 0x03;
const SOCKS5_ATYP_IPV6: u8 = 0x04;
const SOCKS5_SUCCESS: u8 = 0x00;

// Command line configuration structure using clap
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Config {
    /// The address and port where the HTTP proxy server will listen for incoming connections
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    listen: String,

    /// The address and port of the SOCKS proxy server to forward requests to
    #[arg(short, long, default_value = "127.0.0.1:1080")]
    socks: String,

    /// Forward mode: forward raw TCP traffic directly to SOCKS5 (no HTTP protocol handling)
    #[arg(short, long, default_value_t = false)]
    forward: bool,
}

// Main entry point - sets up HTTP proxy server and handles incoming connections
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let config = Config::parse();
    let listener = TcpListener::bind(&config.listen).await?;

    if config.forward {
        info!("TCP forward mode listening on: {}", config.listen);
        info!("Forwarding all traffic to SOCKS5: {}", config.socks);
    } else {
        info!("HTTP proxy listening on: {}", config.listen);
    }

    while let Ok((client, addr)) = listener.accept().await {
        info!("New connection from: {}", addr);
        let socks_addr = config.socks.clone();
        let forward_mode = config.forward;
        tokio::spawn(async move {
            let result = if forward_mode {
                handle_forward_client(client, &socks_addr).await
            } else {
                handle_client(client, &socks_addr).await
            };

            if let Err(e) = result {
                error!("Client handling error: {}", e);
                // Print the error chain
                let mut error_chain = String::new();
                let mut source = e.source();
                while let Some(e) = source {
                    let _ = writeln!(error_chain, "Caused by: {e}");
                    source = e.source();
                }
                if !error_chain.is_empty() {
                    error!("Error chain:\n{}", error_chain);
                }
            }
        });
    }

    Ok(())
}

// Handles individual client connections and processes HTTP requests
#[instrument(skip_all)]
async fn handle_client(mut client: TcpStream, socks_addr: &str) -> Result<(), Box<dyn Error>> {
    let mut buffer = [0u8; 4096];
    let n = client.read(&mut buffer).await.map_err(|e| {
        error!("Failed to read from client: {}", e);
        e
    })?;

    if n == 0 {
        return Err("Client closed connection".into());
    }

    if is_connect_request(&buffer[..n]) {
        // Handle CONNECT tunnel (HTTPS)
        if let Some((host, port)) = parse_connect_request(&buffer[..n]) {
            let socks = connect_socks5(&host, port, socks_addr).await.map_err(|e| {
                error!("Failed to connect via SOCKS5: {}", e);
                e
            })?;
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .map_err(|e| {
                    error!("Failed to send connection established: {}", e);
                    e
                })?;
            proxy_data(client, socks).await?;
        } else {
            warn!("Failed to parse CONNECT request");
            client
                .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
                .await?;
        }
    } else {
        // Handle regular HTTP request
        if let Some((method, host, port, path)) = parse_http_request(&buffer[..n]) {
            let socks = connect_socks5(&host, port, socks_addr).await?;

            // Rewrite request to absolute-form
            let new_request = format!("{method} {path} HTTP/1.1\r\n");
            let mut modified_request = buffer[..n].to_vec();
            modified_request.splice(..first_line_len(&buffer[..n]), new_request.bytes());

            let mut socks = socks;
            socks.write_all(&modified_request).await?;
            proxy_data(client, socks).await?;
        } else {
            client
                .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
                .await?;
        }
    }

    Ok(())
}

fn is_connect_request(buffer: &[u8]) -> bool {
    String::from_utf8_lossy(buffer).starts_with("CONNECT")
}

fn parse_http_request(buffer: &[u8]) -> Option<(String, String, u16, String)> {
    let request = String::from_utf8_lossy(buffer);
    let lines: Vec<&str> = request.split("\r\n").collect();
    let first_line = lines[0];
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() != 3 {
        return None;
    }

    let method = parts[0].to_string();
    let uri = parts[1];

    // Find Host header
    let host_header = lines
        .iter()
        .find(|line| line.to_lowercase().starts_with("host:"))?
        .split(':')
        .nth(1)?
        .trim();

    // Parse host and port from Host header
    let (host, port) = if let Some(idx) = host_header.rfind(':') {
        let (h, p) = host_header.split_at(idx);
        (h.to_string(), p[1..].parse().unwrap_or(80))
    } else {
        (host_header.to_string(), 80)
    };

    Some((method, host, port, uri.to_string()))
}

fn first_line_len(buffer: &[u8]) -> usize {
    if let Some(pos) = buffer.windows(2).position(|w| w == b"\r\n") {
        pos + 2
    } else {
        0
    }
}

// Parses HTTP CONNECT request to extract target host and port
fn parse_connect_request(buffer: &[u8]) -> Option<(String, u16)> {
    // Convert request bytes to string
    let request = String::from_utf8_lossy(buffer);
    // Verify it's a CONNECT request
    if !request.starts_with("CONNECT") {
        return None;
    }

    // Parse request line format: "CONNECT host:port HTTP/1.x"
    let lines: Vec<&str> = request.split("\r\n").collect();
    let first_line = lines[0];
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    // Validate request format
    if parts.len() != 3 {
        return None;
    }

    // Extract host and port
    let addr_parts: Vec<&str> = parts[1].split(':').collect();
    if addr_parts.len() != 2 {
        return None;
    }

    let host = addr_parts[0].to_string();
    let port = addr_parts[1].parse().ok()?;

    Some((host, port))
}

// Establishes connection to SOCKS5 proxy server
#[instrument]
async fn connect_socks5(
    host: &str,
    port: u16,
    socks_addr: &str,
) -> Result<TcpStream, Box<dyn Error>> {
    // Connect to SOCKS5 server
    let mut socks = TcpStream::connect(socks_addr).await?;

    // Perform SOCKS5 handshake
    // Send client greeting: version 5, 1 auth method, no auth required
    socks
        .write_all(&[SOCKS5_VERSION, SOCKS5_AUTH_METHODS, SOCKS5_AUTH_NONE])
        .await?;
    let mut response = [0u8; 2];
    socks.read_exact(&mut response).await?;

    // Send connection request
    // Format: version 5, connect command, reserved byte, dst address, dst port
    let mut request = vec![SOCKS5_VERSION, SOCKS5_CMD_CONNECT, SOCKS5_RSV];

    // Check if host is an IP address
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(ipv4) => {
                request.push(SOCKS5_ATYP_IPV4); // IPv4 address type
                request.extend_from_slice(&ipv4.octets());
            }
            std::net::IpAddr::V6(ipv6) => {
                request.push(SOCKS5_ATYP_IPV6); // IPv6 address type
                request.extend_from_slice(&ipv6.octets());
            }
        }
    } else {
        // Domain name type
        let addr_bytes = host.as_bytes();
        request.push(SOCKS5_ATYP_DOMAIN); // Domain name type
        request.push(addr_bytes.len() as u8);
        request.extend_from_slice(addr_bytes);
    }
    request.extend_from_slice(&port.to_be_bytes());
    socks.write_all(&request).await?;

    // Read connection response header
    let mut header = [0u8; 4];
    socks.read_exact(&mut header).await?;

    if header[1] != SOCKS5_SUCCESS {
        return Err("SOCKS5 connection failed".into());
    }

    // Read variable-length address data based on atyp
    match header[3] {
        SOCKS5_ATYP_IPV4 => {
            // IPv4
            let mut addr = [0u8; 4];
            socks.read_exact(&mut addr).await?;
        }
        SOCKS5_ATYP_DOMAIN => {
            // Domain name
            let mut len = [0u8; 1];
            socks.read_exact(&mut len).await?;
            let mut addr = vec![0u8; len[0] as usize];
            socks.read_exact(&mut addr).await?;
        }
        SOCKS5_ATYP_IPV6 => {
            // IPv6
            let mut addr = [0u8; 16];
            socks.read_exact(&mut addr).await?;
        }
        _ => return Err("Unknown address type".into()),
    }

    // Read port
    let mut port = [0u8; 2];
    socks.read_exact(&mut port).await?;

    Ok(socks)
}

// Handles forward mode - directly forwards TCP traffic to SOCKS5 proxy
#[instrument(skip_all)]
async fn handle_forward_client(client: TcpStream, socks_addr: &str) -> Result<(), Box<dyn Error>> {
    // Simply connect to SOCKS5 and forward all traffic
    let socks = TcpStream::connect(socks_addr).await.map_err(|e| {
        error!("Failed to connect to SOCKS5 server: {}", e);
        e
    })?;

    info!("Forwarding connection to SOCKS5 server");
    proxy_data(client, socks).await
}

// Handles bidirectional data transfer between client and SOCKS connection
#[instrument(skip_all)]
async fn proxy_data(mut client: TcpStream, mut socks: TcpStream) -> Result<(), Box<dyn Error>> {
    match tokio::io::copy_bidirectional(&mut client, &mut socks).await {
        Ok((from_client, from_socks)) => {
            info!(
                "Proxied {} bytes from client, {} bytes from socks",
                from_client, from_socks
            );
            Ok(())
        }
        Err(e) => {
            error!("Proxy data error: {}", e);
            Err(e.into())
        }
    }
}
