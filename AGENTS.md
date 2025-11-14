# 📝 Commit Guidelines
This project follows the [Conventional Commits](https://www.conventionalcommits.org/) specification.

## The commit message should be structured as follows:
```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

## Example
- feat: add streaming support
- fix(parser): handle empty input
- docs: update README with usage example
- chore: initial commit

## Types

- **feat** → a new feature
- **fix** → a bug fix
- **docs** → documentation only changes
- **style** → formatting, missing semicolons, etc. (no code change)
- **refactor** → code change that neither fixes a bug nor adds a feature
- **test** → adding or correcting tests
- **chore** → maintenance tasks (build, tooling, configs, dependencies)

---

# 📚 Crate Documentation

Quick links to documentation for the main crates used in this project:

## Core Dependencies

- **[axum](https://docs.rs/axum/0.8.6/)** (0.8.6) - Web framework for building HTTP servers
- **[tokio](https://docs.rs/tokio/1.48.0/)** (1.48.0) - Async runtime
- **[webrtc](https://docs.rs/webrtc/0.14.0/)** (0.14.0) - WebRTC implementation
- **[retina](https://docs.rs/retina/0.4.15/)** (0.4.15) - RTSP client library

## HTTP & Networking

- **[tower-http](https://docs.rs/tower-http/0.6.6/)** (0.6.6) - Tower middleware for HTTP
- **[url](https://docs.rs/url/2.5.7/)** (2.5.7) - URL parsing

## Utilities

- **[clap](https://docs.rs/clap/4.5.51/)** (4.5.51) - Command-line argument parser
- **[tracing](https://docs.rs/tracing/0.1/)** (0.1) - Structured logging
- **[tracing-subscriber](https://docs.rs/tracing-subscriber/0.3/)** (0.3) - Tracing subscriber implementations
- **[dashmap](https://docs.rs/dashmap/6.1.0/)** (6.1.0) - Concurrent HashMap
- **[uuid](https://docs.rs/uuid/1.18.1/)** (1.18.1) - UUID generation

## Streaming & Serialization

- **[tokio-stream](https://docs.rs/tokio-stream/0.1.17/)** (0.1.17) - Stream utilities for tokio
- **[serde_json](https://docs.rs/serde_json/1.0.145/)** (1.0.145) - JSON serialization
- **[base64](https://docs.rs/base64/0.22.1/)** (0.22.1) - Base64 encoding/decoding

## Error Handling

- **[anyhow](https://docs.rs/anyhow/1.0.100/)** (1.0.100) - Flexible error handling
- **[thiserror](https://docs.rs/thiserror/2.0.17/)** (2.0.17) - Derive macros for error types

## Development

- **[rustyline](https://docs.rs/rustyline/17.0.2/)** (17.0.2) - Readline implementation

### 🐱