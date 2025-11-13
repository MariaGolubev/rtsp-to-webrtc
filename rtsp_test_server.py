#!/usr/bin/env python3
"""
Simple RTSP test server using GStreamer
Provides a test video stream at rtsp://localhost:8554/test
"""
import gi
gi.require_version('Gst', '1.0')
gi.require_version('GstRtspServer', '1.0')
from gi.repository import Gst, GstRtspServer, GLib
import sys

def main():
    # Initialize GStreamer
    Gst.init(None)
    
    # Create RTSP server
    server = GstRtspServer.RTSPServer()
    server.set_service("8554")
    
    # Create media factory for main stream (video + audio)
    factory = GstRtspServer.RTSPMediaFactory()
    
    # Set the pipeline with video and audio
    # Test patterns: smpte, snow, black, white, red, green, blue, checkers-1, checkers-2, 
    #                checkers-4, checkers-8, circular, blink, ball
    # Using G.711 PCMU (mu-law) audio codec which is widely supported in RTSP
    # GOP settings: gop-size=30 means I-frame every 30 frames (1 second at 30fps)
    #               repeat-sequence-header=true inserts SPS/PPS with every IDR frame
    pipeline = (
        "( videotestsrc pattern=ball is-live=true ! "
        "video/x-raw,width=640,height=480,framerate=60/1 ! "
        "clockoverlay time-format=\"%Y-%m-%d %H:%M:%S\" shaded-background=true font-desc=\"Sans, 24\" ! "
        "nvh264enc bitrate=2000 gop-size=30 repeat-sequence-header=true ! "
        "h264parse ! "
        "rtph264pay name=pay0 pt=96 ) "
        "( audiotestsrc is-live=true wave=ticks ! "
        "audio/x-raw,rate=8000,channels=1 ! "
        "audioconvert ! "
        "mulawenc ! "
        "rtppcmupay name=pay1 pt=0 )"
    )
    
    factory.set_launch(pipeline)
    factory.set_shared(True)
    
    # Create second video stream factory (different pattern)
    factory2 = GstRtspServer.RTSPMediaFactory()
    
    pipeline2 = (
        "( videotestsrc pattern=smpte is-live=true ! "
        "video/x-raw,width=1280,height=720,framerate=30/1 ! "
        "clockoverlay time-format=\"%Y-%m-%d %H:%M:%S\" shaded-background=true font-desc=\"Sans, 32\" ! "
        "nvh264enc bitrate=4000 gop-size=30 repeat-sequence-header=true ! "
        "h264parse ! "
        "rtph264pay name=pay0 pt=96 ) "
        "( audiotestsrc is-live=true wave=sine freq=440 ! "
        "audio/x-raw,rate=8000,channels=1 ! "
        "audioconvert ! "
        "alawenc ! "
        "rtppcmapay name=pay1 pt=8 )"
    )
    
    factory2.set_launch(pipeline2)
    factory2.set_shared(True)
    
    # Mount the factories
    mounts = server.get_mount_points()
    mounts.add_factory("/test", factory)
    mounts.add_factory("/test2", factory2)
    
    # Attach server to default main context
    server.attach(None)
    
    print("=" * 70)
    print("RTSP Test Server Started")
    print("=" * 70)
    print("")
    print("Stream 1 (640x480, ball pattern + ticks, H.264 + G.711 PCMU):")
    print(f"  URL: rtsp://localhost:8554/test")
    print(f"  Network: rtsp://localhost:8554/test")
    print("")
    print("Stream 2 (1280x720, SMPTE + sine 440Hz, H.264 + G.711 PCMA):")
    print(f"  URL: rtsp://localhost:8554/test2")
    print(f"  Network: rtsp://localhost:8554/test2")
    print("")
    print("Test with:")
    print("  VLC: vlc rtsp://localhost:8554/test")
    print("  ffplay: ffplay rtsp://localhost:8554/test")
    print("  Your app: ./target/debug/rtsp-to-webrtc --url=rtsp://localhost:8554/test")
    print("")
    print("Press Ctrl+C to stop the server")
    print("=" * 70)
    
    # Run main loop
    loop = GLib.MainLoop()
    try:
        loop.run()
    except KeyboardInterrupt:
        print("\n\nStopping RTSP server...")
        loop.quit()
        print("Server stopped.")

if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        print("\nExiting...")
        sys.exit(0)
