# Rota - High-Performance Proxy Rotation Server

A high-performance proxy rotation server written in Rust, ported from Go. Rota provides intelligent proxy rotation with multiple strategies, health monitoring, and a REST API for management.

## Installation

### Prerequisites

- Rust 1.88 or higher
- PostgreSQL 12 or higher
- Optional: TimescaleDB extension for advanced time-series features

### Build from Source

```bash
git clone <repository-url>
cd rota-rust
cargo build --release
```

## Configuration

Rota is configured via environment variables:

### Database Configuration

```bash
DB_HOST=localhost
DB_PORT=5432
DB_USER=rota
DB_PASSWORD=rota_password
DB_NAME=rota
DB_SSLMODE=disable
DB_MAX_CONNECTIONS=50
DB_MIN_CONNECTIONS=5
```

### Proxy Server Configuration

```bash
PROXY_HOST=0.0.0.0
PROXY_PORT=8000
PROXY_MAX_RETRIES=3
PROXY_CONNECT_TIMEOUT=10
PROXY_REQUEST_TIMEOUT=30
PROXY_ROTATION_STRATEGY=random  # random, round_robin, least_connections, time_based
PROXY_AUTH_ENABLED=false
PROXY_AUTH_USERNAME=
PROXY_AUTH_PASSWORD=
PROXY_RATE_LIMIT_ENABLED=false
PROXY_RATE_LIMIT_PER_SECOND=100
PROXY_RATE_LIMIT_BURST=200
ROTA_EGRESS_PROXY=  # Optional forward proxy for dialing upstream proxies (http://user:pass@host:port or socks5://user:pass@host:port)
```

### API Server Configuration

```bash
API_HOST=0.0.0.0
API_PORT=8001
CORS_ORIGINS=  # Comma-separated list, empty = localhost only
JWT_SECRET=your-secret-key-here
```

### Admin Credentials

```bash
ROTA_ADMIN_USER=admin
ROTA_ADMIN_PASSWORD=admin
```

### Logging

```bash
LOG_LEVEL=info
LOG_FORMAT=json
RUST_LOG=rota=info,tower_http=debug
```

## Database Setup

### PostgreSQL Setup

```bash
# Create database and user
createdb rota
psql rota -c "CREATE USER rota WITH PASSWORD 'rota_password';"
psql rota -c "GRANT ALL PRIVILEGES ON DATABASE rota TO rota;"
```

### Optional: TimescaleDB Extension

```bash
psql rota -c "CREATE EXTENSION IF NOT EXISTS timescaledb;"
```

### Migrations

Migrations run automatically on startup. The server will create all necessary tables and indexes.

## Usage

### Running the Server

```bash
# Development
cargo run

# Production (with optimizations)
cargo run --release
```

### Using the Proxy

Configure your HTTP client to use the proxy:

```bash
# Using curl
curl -x http://localhost:8000 https://api.ipify.org

# With authentication
curl -x http://username:password@localhost:8000 https://api.ipify.org
```

### Managing Proxies

#### Add a Proxy

```bash
curl -X POST http://localhost:8001/api/proxies \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{
    "address": "proxy.example.com:8080",
    "protocol": "http",
    "username": "user",
    "password": "pass"
  }'
```

#### List Proxies

```bash
curl http://localhost:8001/api/proxies \
  -H "Authorization: Bearer <token>"
```

#### Update Proxy

```bash
curl -X PUT http://localhost:8001/api/proxies/1 \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{
    "status": "active"
  }'
```

#### Delete Proxy

```bash
curl -X DELETE http://localhost:8001/api/proxies/1 \
  -H "Authorization: Bearer <token>"
```

## API Endpoints

### Authentication

- `POST /api/auth/login` - Login and get JWT token

### Proxies

- `GET /api/proxies` - List proxies with pagination
- `POST /api/proxies` - Create a new proxy
- `GET /api/proxies/:id` - Get proxy details
- `PUT /api/proxies/:id` - Update proxy
- `DELETE /api/proxies/:id` - Delete proxy
- `POST /api/proxies/bulk` - Bulk create proxies
- `DELETE /api/proxies/bulk` - Bulk delete proxies

### Dashboard

- `GET /api/dashboard/stats` - Get system and proxy statistics
- `GET /api/dashboard/health` - Get service health status
- `WS /api/dashboard/ws` - WebSocket for real-time updates

### Logs

- `GET /api/logs` - Get request logs with pagination
- `DELETE /api/logs` - Clear logs

### Settings

- `GET /api/settings` - Get all settings
- `PUT /api/settings` - Update settings

## Development


### Running Tests

```bash
cargo test
```

### Code Formatting

```bash
cargo fmt
```

### Linting

```bash
cargo clippy
```
