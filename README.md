# Prusa RTSP Streamer

Stream from your RTSP camera to prusa connect.

## Running

If you wish to run as a service (eg: using systemd on a prusalink device).
1. Copy `prusa-rtsp-streamer` to `/usr/local/bin`
2. Copy `prusa-rtsp-streamer.service` to `/etc/systemd/system/`
3. Populate your config at `/etc/prusa-rtsp-streamer/config.toml`
4. Test the service: `systemctl start prusa-rtsp-streamer`
5. Enable the service on boot: `systemctl enable prusa-rtsp-streamer`

### Config
```toml
snapshot_interval = 30

[[camera]]
token = "mnbdNm8ATLmJLMSvaWZh" # "custom camera" from prusa connect website
url = "rtsp://192.168.0.2:8080/h264.sdp"

[[camera]]
token = "BTGWx7tJRQGcZzh8r99r" 
url = "rtsp://192.168.0.4:8080/stream1"
username = "username"
password = "password"
```

## Building

x64:
```sh
sudo apt install nasm
cargo build --release
```

Prusalink:
```sh
cargo install cross
cross build --release --target armv7-unknown-linux-gnueabihf
```