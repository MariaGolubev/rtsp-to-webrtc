# Обновлённое логирование

## Что теперь логируется

### 1. HTTP Запросы (через TraceLayer)
```
INFO http_request{method=POST uri=/whep version=HTTP/1.1}: response status=201 Created latency_ms=45
INFO http_request{method=DELETE uri=/whep/resource/abc123 version=HTTP/1.1}: response status=204 No Content latency_ms=12
```

### 2. UDP RTP пакеты
```
INFO Received 100 RTP packets (last from 127.0.0.1:54321), broadcasting to 1 tracks
INFO Received 200 RTP packets (last from 127.0.0.1:54321), broadcasting to 2 tracks
```

### 3. Создание/удаление сессий
```
INFO ✅ Session created: abc12345 | Sessions: 1 | Tracks: 1
INFO 🗑️  Session deleted: abc12345 | Remaining: 0
```

### 4. Ошибки
```
WARN Invalid Content-Type: 'text/plain'
WARN ⚠️  Session not found: xyz98765
ERROR Failed to read body: connection closed
ERROR video_track write error: closed pipe
```

## Запуск

### Обычный режим (INFO)
```bash
cargo run
```

Пример вывода:
```
2024-11-12T10:30:15Z  INFO Starting RTSP to WebRTC server
2024-11-12T10:30:15Z  INFO UDP listener started on 127.0.0.1:5004
2024-11-12T10:30:15Z  INFO 🚀 WHEP server started on http://localhost:8080
2024-11-12T10:31:00Z  INFO http_request{method=POST uri=/whep version=HTTP/1.1}: response status=201 Created latency_ms=45
2024-11-12T10:31:00Z  INFO ✅ Session created: a1b2c3d4 | Sessions: 1 | Tracks: 1
2024-11-12T10:31:05Z  INFO Received 100 RTP packets (last from 127.0.0.1:54321), broadcasting to 1 tracks
2024-11-12T10:35:00Z  INFO http_request{method=DELETE uri=/whep/resource/a1b2c3d4 version=HTTP/1.1}: response status=204 No Content latency_ms=12
2024-11-12T10:35:00Z  INFO 🗑️  Session deleted: a1b2c3d4 | Remaining: 0
```

### Debug режим (больше деталей)
```bash
RUST_LOG=debug cargo run
```

### Только ошибки
```bash
RUST_LOG=error cargo run
```

## Что убрали

❌ Лишние логи на каждый шаг создания сессии:
- "📥 Processing WHEP offer request"
- "🔗 Created new PeerConnection"
- "📹 Created video track with VP8 codec"
- "➕ Added video track to PeerConnection"
- "📝 Registered video track for session"
- "📝 Set remote description"
- "📝 Created answer"
- "📝 Set local description"
- "📤 Sending SDP answer"

✅ Оставили только важное:
- HTTP запросы с методом, URI, статусом и задержкой
- Создание/удаление сессий с количеством активных
- Статистика RTP пакетов (каждые 100 пакетов)
- Ошибки

## Преимущества

1. **Читаемость** - меньше шума в логах
2. **Производительность** - меньше операций логирования
3. **Информативность** - видно только важные события
4. **HTTP метрики** - автоматический трейсинг всех HTTP запросов с задержкой
