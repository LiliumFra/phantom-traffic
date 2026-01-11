# Phantom Traffic 🌐

> **Ultra-fast traffic generation bot for Termux/Android with AADS & Monetag support**

![Version](https://img.shields.io/badge/version-36.0.0-blue)
![Rust](https://img.shields.io/badge/rust-1.75+-orange)
![Platform](https://img.shields.io/badge/platform-Termux%20%7C%20Linux-green)

## ✨ Features

| Feature | Description |
|---------|-------------|
| 🎯 **AADS Support** | Auto-detect `data-aa` attributes and iframe ads |
| 💰 **Monetag Support** | Handle JS popunder scripts automatically |
| 🧅 **Tor Integration** | Multi-instance Tor rotation for anonymity |
| 🌐 **60+ Proxy Sources** | Massive proxy pool with auto-validation |
| 📱 **Termux Native** | Optimized for Android devices |
| 🎭 **12 Device Profiles** | Realistic fingerprinting (Pixel, iPhone, Galaxy, etc.) |
| ⚡ **Lazy Regex** | Pre-compiled patterns for maximum performance |
| 📊 **Real-time Stats** | Live dashboard with ad-specific metrics |

## 🚀 One-Command Install (Termux)

```bash
pkg update && pkg install -y git rust tor
git clone https://github.com/YOUR_USER/phantom-traffic.git
cd phantom-traffic && cargo build --release
./target/release/icecold --help
```

Or use the installer:

```bash
bash install.sh
```

## 📱 Usage

### Interactive Mode

```bash
icecold
# Paste URL or HTML, then press Enter twice
```

### Direct Mode

```bash
# AADS
icecold --url "https://ad.a-ads.com/YOUR_AD_ID"

# With custom settings
icecold --url "URL" --workers 30 --tor-instances 2
```

### CLI Options

| Option | Default | Description |
|--------|---------|-------------|
| `-u, --url` | - | Target URL or HTML |
| `-w, --workers` | 50 | Concurrent workers |
| `-r, --click-rate` | 0.055 | Click rate (0.0-1.0) |
| `--no-tor` | false | Disable Tor |
| `--tor-instances` | 4 | Number of Tor nodes |
| `--delay` | 3500 | Base delay (ms) |
| `-q, --quiet` | false | Minimal output |

## 📊 Stats Dashboard

```
╔════════════════ PHANTOM v36 [TERMUX] ════════════════╗
║ Workers:   50 │ Proxies:  1200 │ Tor:  ON   ║
╠══════════════════════════════════════════════════════╣
║  IMP:     1234 │ CLK:    68 │ CTR:  5.51%    ║
║  ERR:       45 │ RPM:   120 │ ERR%:  3.5%    ║
╠══════════════════════════════════════════════════════╣
║  AADS:   1100 │ MNTG:   134 │ POP:    890   ║
╚══════════════════════════════════════════════════════╝
```

## ⚙️ Configuration

Create `~/.config/icecold/config.toml`:

```toml
target_url = "https://your-target.com"
workers = 30
click_rate = 0.055
tor_instances = 2
```

## 🔧 Performance Tips

### For Low-End Devices (Tab A9, etc.)

```bash
icecold --workers 20 --tor-instances 2
```

### For Maximum Performance

```bash
icecold --workers 100 --tor-instances 6
```

### Keep Running in Background

```bash
termux-wake-lock
nohup icecold --url "URL" &
```

## 📁 File Locations

| File | Path |
|------|------|
| Binary | `$PREFIX/bin/icecold` |
| Config | `~/.config/icecold/config.toml` |
| Gold Proxies | `~/.icecold/gold_proxies.json` |
| Stats | `~/.icecold/last_stats.json` |

## 🛠️ Development

```bash
# Build
cargo build --release

# Run tests
cargo test

# Check for errors
cargo check
```

## 📜 License

MIT License - Use responsibly.

---

**Made with 🦀 Rust for Termux**
