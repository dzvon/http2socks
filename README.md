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
- `-f, --forward`: Forward mode - forward raw TCP traffic directly to SOCKS5 (no HTTP protocol handling)

## Examples

### HTTP Proxy Mode (default)

```bash
# Use with Tor
./http2socks --socks 127.0.0.1:9050

# Configure your browser to use HTTP proxy at 127.0.0.1:8080
# Or use with curl:
curl --proxy http://127.0.0.1:8080 https://example.com
```

### Forward Mode

Forward mode listens on a TCP port and forwards all traffic directly to the SOCKS5 proxy server without any HTTP protocol handling:

```bash
# Forward all traffic to SOCKS5 proxy
./http2socks --forward --listen 127.0.0.1:8080 --socks 127.0.0.1:1080

# This creates a simple TCP tunnel - any client connecting to 127.0.0.1:8080
# will have their traffic forwarded directly to the SOCKS5 server at 127.0.0.1:1080
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
