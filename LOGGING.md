# Примеры логирования

## Запуск с различными уровнями логирования

### По умолчанию (INFO уровень)
```bash
cargo run
```

### DEBUG уровень (подробные логи)
```bash
RUST_LOG=debug cargo run
```

### TRACE уровень (максимально подробные логи)
```bash
RUST_LOG=trace cargo run
```

### Только ошибки
```bash
RUST_LOG=error cargo run
```

### Фильтрация по модулям
```bash
# Только логи от webrtc
RUST_LOG=webrtc=debug cargo run

# Логи от нескольких модулей
RUST_LOG=rtsp_to_webrtc=debug,webrtc=info cargo run

# Отключить логи от tower_http
RUST_LOG=rtsp_to_webrtc=debug,tower_http=error cargo run
```

## Примеры логов

### При запуске сервера
```
2024-11-12T10:30:15.123456Z  INFO Starting RTSP to WebRTC server
2024-11-12T10:30:15.234567Z  INFO UDP listener started on 127.0.0.1:5004
2024-11-12T10:30:15.234578Z  INFO Send RTP packets to this address using GStreamer or ffmpeg
2024-11-12T10:30:15.345678Z  INFO 🚀 WHEP server started on http://localhost:8080
2024-11-12T10:30:15.345689Z  INFO 📡 POST SDP offers to http://localhost:8080/whep
2024-11-12T10:30:15.345690Z  INFO 🗑️  DELETE sessions at http://localhost:8080/whep/resource/{id}
```

### При подключении клиента
```
2024-11-12T10:31:00.123456Z  INFO POST /whep
2024-11-12T10:31:00.234567Z  INFO 📥 Received WHEP offer request
2024-11-12T10:31:00.345678Z  INFO ✅ Created new WHEP session: a1b2c3d4-e5f6-7890-abcd-ef1234567890 (total active: 1)
```

### При получении RTP пакетов
```
2024-11-12T10:31:05.123456Z  INFO Received 100 RTP packets (last from 127.0.0.1:54321), broadcasting to 1 tracks
2024-11-12T10:31:10.234567Z  INFO Received 200 RTP packets (last from 127.0.0.1:54321), broadcasting to 1 tracks
```

### При отключении клиента
```
2024-11-12T10:35:00.123456Z  INFO DELETE /whep/resource/a1b2c3d4-e5f6-7890-abcd-ef1234567890
2024-11-12T10:35:00.234567Z  INFO 🗑️  Delete request for session: a1b2c3d4-e5f6-7890-abcd-ef1234567890
2024-11-12T10:35:00.345678Z  INFO ✅ Session a1b2c3d4-e5f6-7890-abcd-ef1234567890 closed (remaining active: 0)
```

### При ошибках
```
2024-11-12T10:40:00.123456Z  WARN ⚠️  Session unknown-id not found
2024-11-12T10:40:05.234567Z  ERROR video_track write error: closed pipe
2024-11-12T10:40:10.345678Z  ERROR UDP recv error: connection reset
```

## HTTP запросы (через tower-http TraceLayer)

### Формат логов HTTP
```
2024-11-12T10:31:00.123456Z  INFO request{method=POST uri=/whep version=HTTP/1.1}: tower_http::trace::on_request: started processing request
2024-11-12T10:31:00.234567Z  INFO request{method=POST uri=/whep version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=111 ms status=201
```

## Полезные команды

### Сохранение логов в файл
```bash
cargo run 2>&1 | tee server.log
```

### Логи с временными метками
```bash
RUST_LOG=debug cargo run 2>&1 | ts '[%Y-%m-%d %H:%M:%S]'
```

### Поиск в логах
```bash
cargo run 2>&1 | grep "session"
cargo run 2>&1 | grep -E "(ERROR|WARN)"
```

## Настройка формата логов

В коде можно изменить формат логирования:

```rust
// Компактный формат
tracing_subscriber::fmt()
    .compact()
    .init();

// С полным путём к файлу
tracing_subscriber::fmt()
    .with_file(true)
    .with_line_number(true)
    .init();

// JSON формат
tracing_subscriber::fmt()
    .json()
    .init();
```
