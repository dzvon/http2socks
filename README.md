# http2socks

A simple HTTP to SOCKS5 proxy bridge written in Rust.

## Description

Routes HTTP and HTTPS traffic through a SOCKS5 proxy server. Accepts standard HTTP proxy requests and forwards them via SOCKS5.

## Installation

```bash
cargo build --release
```

## Usage

```bash
# Default: listen on 127.0.0.1:8080, forward to 127.0.0.1:1080
./http2socks

# Custom addresses
./http2socks --listen 0.0.0.0:3128 --socks 127.0.0.1:9050
```

### Options

- `-l, --listen <ADDRESS>`: HTTP proxy listen address (default: 127.0.0.1:8080)
- `-s, --socks <ADDRESS>`: SOCKS5 proxy server address (default: 127.0.0.1:1080)

## Examples

```bash
# Use with Tor
./http2socks --socks 127.0.0.1:9050

# Configure your browser to use HTTP proxy at 127.0.0.1:8080
# Or use with curl:
curl --proxy http://127.0.0.1:8080 https://example.com
```

## Logging

```bash
RUST_LOG=debug ./http2socks  # Enable debug logging
```

## Features

- HTTP/HTTPS support via CONNECT tunneling
- Async I/O with Tokio
- IPv4/IPv6 and domain name support
- No authentication (forwards to SOCKS5 as-is)