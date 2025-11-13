# RTSP to WebRTC

Проект для передачи видео через UDP RTP в WebRTC с использованием WHEP протокола.

## Как работает

1. Сервер запускает WHEP endpoint на `http://localhost:8080/whep`
2. UDP listener принимает RTP пакеты на порту `5004`
3. RTP пакеты транслируются всем активным WebRTC соединениям
4. Браузер подключается через WHEP и получает видео поток

## Запуск

### 1. Запустить сервер

```bash
cargo run
```

**С подробным логированием:**

```bash
# DEBUG уровень
RUST_LOG=debug cargo run

# TRACE уровень (максимально подробно)
RUST_LOG=trace cargo run
```

Подробнее о логировании см. [LOGGING.md](LOGGING.md)

Сервер запустится на:
- HTTP: `http://localhost:8080`
- UDP RTP: `127.0.0.1:5004`

### 2. Открыть браузер

Откройте `index.html` в браузере или перейдите на `http://localhost:8080` (если настроена статическая раздача).

### 3. Отправить видео через UDP

Используйте GStreamer или ffmpeg для отправки RTP пакетов:

#### GStreamer

```bash
gst-launch-1.0 videotestsrc ! video/x-raw,width=640,height=480,format=I420 ! \
  vp8enc error-resilient=partitions keyframe-max-dist=10 auto-alt-ref=true cpu-used=5 deadline=1 ! \
  rtpvp8pay ! udpsink host=127.0.0.1 port=5004
```

#### ffmpeg

```bash
ffmpeg -re -f lavfi -i testsrc=size=640x480:rate=30 -vcodec libvpx \
  -cpu-used 5 -deadline 1 -g 10 -error-resilient 1 -auto-alt-ref 1 \
  -f rtp rtp://127.0.0.1:5004?pkt_size=1200
```

#### Реальная камера через ffmpeg

```bash
ffmpeg -i /dev/video0 -vcodec libvpx -cpu-used 5 -deadline 1 \
  -g 10 -error-resilient 1 -auto-alt-ref 1 \
  -f rtp rtp://127.0.0.1:5004?pkt_size=1200
```

## API Endpoints

### POST /whep
Создаёт новое WebRTC соединение.

**Request:**
- Content-Type: `application/sdp`
- Body: SDP offer от браузера

**Response:**
- Status: 201 Created
- Content-Type: `application/sdp`
- Header: `Location: /resource/{id}`
- Body: SDP answer

### DELETE /whep/resource/{id}
Закрывает WebRTC соединение.

**Response:**
- Status: 204 No Content (успешно)
- Status: 404 Not Found (сессия не найдена)

## Особенности

- Поддержка множественных одновременных подключений
- Один UDP listener для всех соединений
- Автоматическая очистка закрытых соединений
- Использование VP8 кодека для видео
- WHEP (WebRTC-HTTP Egress Protocol) для простого подключения
- Подробное логирование с tracing (HTTP запросы, WebRTC сессии, RTP пакеты)
- Цветной вывод логов с эмодзи для удобства
- WHEP (WebRTC-HTTP Egress Protocol) для простого подключения

## Требования

- Rust 1.70+
- GStreamer или ffmpeg для отправки видео
- Современный браузер с поддержкой WebRTC

## Примечания

Браузер должен использовать WHEP-совместимый клиент (например, `@eyevinn/whep-video-component`).
