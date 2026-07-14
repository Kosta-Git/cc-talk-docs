set dotenv-load

[private]
default:
    @just --list

# Start the STDIO server
start-mcp:
    cargo run -p api -- stdio

# Start the HTTP server
start-http:
    cargo run -p api -- http

# Build the database from the pdf docs
build-database:
    cargo run -p database-seeder -- ./docs

# Build release artifacts
build-release:
    cargo build --release

# Build Docker image
package:
    docker build -t cc-talk-docs:latest .
