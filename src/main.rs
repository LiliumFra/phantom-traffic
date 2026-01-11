// ═══════════════════════════════════════════════════════════════════════════════
// PHANTOM TRAFFIC v37 - RUST TURBO EDITION
// Ultra-optimized traffic bot with human behavior simulation
// 8 Ad Networks | Anti-Detection | Termux Native
// ═══════════════════════════════════════════════════════════════════════════════

use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::{HashSet, BinaryHeap, HashMap};
use std::cmp::Ordering as CmpOrdering;
use std::path::PathBuf;

use tokio::sync::{RwLock, Semaphore};
use tokio::time::sleep;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use clap::Parser;
use colored::Colorize;
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use anyhow::{Result, Context};
use once_cell::sync::Lazy;

// ═══════════════════════════════════════════════════════════════════════════════
// LAZY COMPILED REGEX (compile once, reuse everywhere)
// ═══════════════════════════════════════════════════════════════════════════════
static PROXY_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d{2,5}").unwrap()
});

static AADS_DATA_AA_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"data-aa=["']?(\d+)["']?"#).unwrap()
});

static AADS_URL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:https?:)?//ad\.a-ads\.com/\d+"#).unwrap()
});

static SRC_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"src\s*=\s*["']((?:https?:)?//[^"']+)["']"#).unwrap()
});

static HREF_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"href\s*=\s*["'](https?://[^"']+)["']"#).unwrap()
});

// Progress bar styles
use indicatif::{ProgressBar, ProgressStyle};

// ═══════════════════════════════════════════════════════════════════════════════
// CLI ARGUMENTS
// ═══════════════════════════════════════════════════════════════════════════════
#[derive(Parser, Debug)]
#[command(name = "icecold")]
#[command(author = "IceCold Team")]
#[command(version = "35.0")]
#[command(about = "Ultra-optimized traffic generation bot for Termux/Android", long_about = None)]
struct Args {
    /// Target URL (or paste HTML with iframe)
    #[arg(short, long)]
    url: Option<String>,

    /// Number of concurrent workers
    #[arg(short, long, default_value_t = 50)]
    workers: usize,

    /// Click rate (0.0 to 1.0)
    #[arg(short = 'r', long, default_value_t = 0.055)]
    click_rate: f32,

    /// Disable Tor usage
    #[arg(long, default_value_t = false)]
    no_tor: bool,

    /// Number of Tor instances
    #[arg(long, default_value_t = 4)]
    tor_instances: usize,

    /// Config file path
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Base delay between requests (ms)
    #[arg(long, default_value_t = 3500)]
    delay: u64,

    /// Request timeout (seconds)
    #[arg(long, default_value_t = 35)]
    timeout: u64,

    /// Proxy refresh interval (minutes)
    #[arg(long, default_value_t = 12)]
    refresh: u64,

    /// Run in quiet mode (less output)
    #[arg(short, long, default_value_t = false)]
    quiet: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONFIGURATION
// ═══════════════════════════════════════════════════════════════════════════════
#[derive(Debug, Deserialize, Serialize)]
struct Config {
    target_url: Option<String>,
    workers: Option<usize>,
    click_rate: Option<f32>,
    no_tor: Option<bool>,
    tor_instances: Option<usize>,
    delay_ms: Option<u64>,
    timeout_sec: Option<u64>,
    refresh_mins: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_url: None,
            workers: Some(50),
            click_rate: Some(0.055),
            no_tor: Some(false),
            tor_instances: Some(4),
            delay_ms: Some(3500),
            timeout_sec: Some(35),
            refresh_mins: Some(12),
        }
    }
}

async fn load_config(path: Option<PathBuf>) -> Config {
    if let Some(p) = path {
        if let Ok(content) = tokio::fs::read_to_string(&p).await {
            if let Ok(cfg) = toml::from_str(&content) {
                println!("{}", format!("[✓] Config loaded from {:?}", p).green());
                return cfg;
            }
        }
    }
    
    // Try default locations
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let default_paths = vec![
        PathBuf::from("icecold.toml"),
        PathBuf::from(format!("{}/.config/icecold/config.toml", home)),
        PathBuf::from(format!("{}/icecold.toml", home)),
    ];
    
    for p in default_paths {
        if let Ok(content) = tokio::fs::read_to_string(&p).await {
            if let Ok(cfg) = toml::from_str(&content) {
                println!("{}", format!("[✓] Config loaded from {:?}", p).green());
                return cfg;
            }
        }
    }
    
    Config::default()
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════
const BASE_SOCKS_PORT: u16 = 9050;
const MAX_PROXIES_CACHE: usize = 40000;
const VALIDATION_TIMEOUT_MS: u64 = 5000;
const VALIDATION_BATCH_SIZE: usize = 100;
const GOLD_PROXIES_FILE: &str = "gold_proxies.json";
const MIN_GOLD_SPEED_MS: u64 = 3000;
const MAX_RETRIES: u32 = 3;

// ═══════════════════════════════════════════════════════════════════════════════
// PROXY SOURCES (60+ SOURCES)
// ═══════════════════════════════════════════════════════════════════════════════
const PROXY_SOURCES: &[&str] = &[
    // API Sources
    "https://api.proxyscrape.com/v2/?request=getproxies&protocol=http&timeout=9000&country=all&ssl=all&anonymity=elite",
    "https://api.proxyscrape.com/v2/?request=getproxies&protocol=socks5&timeout=9000&country=all",
    "https://api.proxyscrape.com/v2/?request=getproxies&protocol=socks4&timeout=9000&country=all",
    "https://www.proxy-list.download/api/v1/get?type=http&anon=elite",
    "https://www.proxy-list.download/api/v1/get?type=socks5",
    "https://www.proxy-list.download/api/v1/get?type=socks4",
    "https://proxylist.geonode.com/api/proxy-list?limit=500&page=1&sort_by=lastChecked&sort_type=desc",
    "https://proxylist.geonode.com/api/proxy-list?limit=500&page=2&sort_by=speed&sort_type=asc",
    
    // TheSpeedX
    "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/http.txt",
    "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks5.txt",
    "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks4.txt",
    
    // ShiftyTR
    "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/http.txt",
    "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/socks5.txt",
    "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/socks4.txt",
    
    // monosans
    "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/http.txt",
    "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/socks5.txt",
    "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/socks4.txt",
    
    // hookzof
    "https://raw.githubusercontent.com/hookzof/socks5_list/master/proxy.txt",
    
    // prxchk
    "https://raw.githubusercontent.com/prxchk/proxy-list/main/http.txt",
    "https://raw.githubusercontent.com/prxchk/proxy-list/main/socks5.txt",
    "https://raw.githubusercontent.com/prxchk/proxy-list/main/socks4.txt",
    
    // zloi-user
    "https://raw.githubusercontent.com/zloi-user/hideip.me/main/http.txt",
    "https://raw.githubusercontent.com/zloi-user/hideip.me/main/socks5.txt",
    "https://raw.githubusercontent.com/zloi-user/hideip.me/main/socks4.txt",
    
    // roosterkid
    "https://raw.githubusercontent.com/roosterkid/openproxylist/main/HTTPS_RAW.txt",
    "https://raw.githubusercontent.com/roosterkid/openproxylist/main/SOCKS5_RAW.txt",
    "https://raw.githubusercontent.com/roosterkid/openproxylist/main/SOCKS4_RAW.txt",
    
    // jetkai
    "https://raw.githubusercontent.com/jetkai/proxy-list/main/online-proxies/txt/proxies-http.txt",
    "https://raw.githubusercontent.com/jetkai/proxy-list/main/online-proxies/txt/proxies-socks5.txt",
    "https://raw.githubusercontent.com/jetkai/proxy-list/main/online-proxies/txt/proxies-socks4.txt",
    
    // mmpx12
    "https://raw.githubusercontent.com/mmpx12/proxy-list/master/http.txt",
    "https://raw.githubusercontent.com/mmpx12/proxy-list/master/socks5.txt",
    "https://raw.githubusercontent.com/mmpx12/proxy-list/master/socks4.txt",
    
    // sunny9577
    "https://raw.githubusercontent.com/sunny9577/proxy-scraper/master/proxies.txt",
    
    // ObcbO
    "https://raw.githubusercontent.com/ObcbO/getproxy/master/file/http.txt",
    "https://raw.githubusercontent.com/ObcbO/getproxy/master/file/socks5.txt",
    "https://raw.githubusercontent.com/ObcbO/getproxy/master/file/socks4.txt",
    
    // vakhov
    "https://raw.githubusercontent.com/vakhov/fresh-proxy-list/master/http.txt",
    "https://raw.githubusercontent.com/vakhov/fresh-proxy-list/master/socks5.txt",
    "https://raw.githubusercontent.com/vakhov/fresh-proxy-list/master/socks4.txt",
    
    // rdavydov
    "https://raw.githubusercontent.com/rdavydov/proxy-list/main/proxies/http.txt",
    "https://raw.githubusercontent.com/rdavydov/proxy-list/main/proxies/socks5.txt",
    "https://raw.githubusercontent.com/rdavydov/proxy-list/main/proxies/socks4.txt",
    
    // UptimerBot
    "https://raw.githubusercontent.com/UptimerBot/proxy-list/main/proxies/http.txt",
    "https://raw.githubusercontent.com/UptimerBot/proxy-list/main/proxies/socks5.txt",
    "https://raw.githubusercontent.com/UptimerBot/proxy-list/main/proxies/socks4.txt",
    
    // caliphdev
    "https://raw.githubusercontent.com/caliphdev/Proxy-List/master/http.txt",
    "https://raw.githubusercontent.com/caliphdev/Proxy-List/master/socks5.txt",
    
    // ErcinDedeoglu
    "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/http.txt",
    "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/socks5.txt",
    "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/socks4.txt",
    
    // elliottophellia
    "https://raw.githubusercontent.com/elliottophellia/yakumo/master/results/http/global/http_checked.txt",
    "https://raw.githubusercontent.com/elliottophellia/yakumo/master/results/socks5/global/socks5_checked.txt",
    "https://raw.githubusercontent.com/elliottophellia/yakumo/master/results/socks4/global/socks4_checked.txt",
    
    // proxifly
    "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/protocols/http/data.txt",
    "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/protocols/socks5/data.txt",
    "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/protocols/socks4/data.txt",
    
    // tuanminpay
    "https://raw.githubusercontent.com/tuanminpay/live-proxy/master/http.txt",
    "https://raw.githubusercontent.com/tuanminpay/live-proxy/master/socks5.txt",
    "https://raw.githubusercontent.com/tuanminpay/live-proxy/master/socks4.txt",
    
    // clarketm
    "https://raw.githubusercontent.com/clarketm/proxy-list/master/proxy-list-raw.txt",
    
    // fate0
    "https://raw.githubusercontent.com/fate0/proxylist/master/proxy.list",
    
    // almroot
    "https://raw.githubusercontent.com/almroot/proxylist/master/list.txt",
    
    // hendrikbgr
    "https://raw.githubusercontent.com/hendrikbgr/Free-Proxy-Repo/master/proxy_list.txt",
    
    // saschazesiger
    "https://raw.githubusercontent.com/saschazesiger/Free-Proxies/master/proxies/http.txt",
    "https://raw.githubusercontent.com/saschazesiger/Free-Proxies/master/proxies/socks5.txt",
    
    // HyperBeats
    "https://raw.githubusercontent.com/HyperBeats/proxy-list/main/http.txt",
    "https://raw.githubusercontent.com/HyperBeats/proxy-list/main/socks5.txt",
    
    // MuRongPIG
    "https://raw.githubusercontent.com/MuRongPIG/Proxy-Master/main/http.txt",
    "https://raw.githubusercontent.com/MuRongPIG/Proxy-Master/main/socks5.txt",
    
    // === NEW SOURCES (15+ more) ===
    
    // im-razvan
    "https://raw.githubusercontent.com/im-razvan/proxy_list/main/http.txt",
    "https://raw.githubusercontent.com/im-razvan/proxy_list/main/socks5.txt",
    
    // Zaeem20
    "https://raw.githubusercontent.com/Zaeem20/FREE_PROXIES_LIST/master/http.txt",
    "https://raw.githubusercontent.com/Zaeem20/FREE_PROXIES_LIST/master/socks5.txt",
    
    // zevtyardt
    "https://raw.githubusercontent.com/zevtyardt/proxy-list/main/http.txt",
    "https://raw.githubusercontent.com/zevtyardt/proxy-list/main/socks5.txt",
    
    // officialputuid
    "https://raw.githubusercontent.com/officialputuid/KangProxy/KangProxy/http/http.txt",
    "https://raw.githubusercontent.com/officialputuid/KangProxy/KangProxy/socks5/socks5.txt",
    
    // yemixzy
    "https://raw.githubusercontent.com/yemixzy/proxy-list/main/proxies/http.txt",
    "https://raw.githubusercontent.com/yemixzy/proxy-list/main/proxies/socks5.txt",
    
    // rx443
    "https://raw.githubusercontent.com/rx443/proxy-list/main/online/http.txt",
    "https://raw.githubusercontent.com/rx443/proxy-list/main/online/socks5.txt",
    
    // Anonym0usWork1221
    "https://raw.githubusercontent.com/Anonym0usWork1221/Free-Proxies/main/proxy_files/http_proxies.txt",
    "https://raw.githubusercontent.com/Anonym0usWork1221/Free-Proxies/main/proxy_files/socks5_proxies.txt",
    
    // proxy4parsing
    "https://raw.githubusercontent.com/proxy4parsing/proxy-list/main/http.txt",
    
    // berkay-digital
    "https://raw.githubusercontent.com/berkay-digital/Proxy-Starter/main/http.txt",
    "https://raw.githubusercontent.com/berkay-digital/Proxy-Starter/main/socks5.txt",
];

const SEARCH_ENGINES: &[&str] = &[
    "https://www.google.com/search?q=crypto+exchange",
    "https://www.google.com/search?q=dolar+blue+argentina",
    "https://www.google.com/search?q=usdt+p2p+argentina",
    "https://www.bing.com/search?q=dolar+blue+hoy",
    "https://www.bing.com/search?q=cotizacion+dolar",
    "https://duckduckgo.com/?q=usdt+p2p",
    "https://duckduckgo.com/?q=cripto+argentina",
    "https://search.yahoo.com/search?p=dolar+mep",
    "https://www.ecosia.org/search?q=dolar+ccl",
];

const INTERNAL_PAGES: &[&str] = &[
    "https://www.dolarix.xyz/?ref=rust_opt",
    "https://www.dolarix.xyz/dolar-crypto",
    "https://www.dolarix.xyz/noticias",
    "https://www.dolarix.xyz/calculadora",
    "https://www.dolarix.xyz/historico",
];

// ═══════════════════════════════════════════════════════════════════════════════
// DEVICE PROFILES WITH CLIENT HINTS
// ═══════════════════════════════════════════════════════════════════════════════
struct DeviceProfile {
    ua_template: &'static str,
    model: &'static str,
    width: u32,
    platform: &'static str,
    mobile: bool,
    brands: &'static str,
}

const PROFILES: &[DeviceProfile] = &[
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Linux; Android 13; SM-X110) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Mobile Safari/537.36",
        model: "SM-X110",
        width: 800,
        platform: "Android",
        mobile: true,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Linux; Android 14; SM-X210) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Mobile Safari/537.36",
        model: "SM-X210",
        width: 1200,
        platform: "Android",
        mobile: true,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Mobile Safari/537.36",
        model: "Pixel 8",
        width: 1080,
        platform: "Android",
        mobile: true,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Linux; Android 14; Pixel 7a) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Mobile Safari/537.36",
        model: "Pixel 7a",
        width: 1080,
        platform: "Android",
        mobile: true,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Linux; Android 13; Redmi Note 12) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Mobile Safari/537.36",
        model: "Redmi Note 12",
        width: 1080,
        platform: "Android",
        mobile: true,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Linux; Android 14; SM-A546B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Mobile Safari/537.36",
        model: "Galaxy A54",
        width: 1080,
        platform: "Android",
        mobile: true,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1",
        model: "iPhone 15",
        width: 390,
        platform: "iOS",
        mobile: true,
        brands: r#""Safari";v="17","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1",
        model: "iPhone 14",
        width: 390,
        platform: "iOS",
        mobile: true,
        brands: r#""Safari";v="16","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Safari/537.36",
        model: "Desktop Win10",
        width: 1920,
        platform: "Windows",
        mobile: false,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Windows NT 11.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Safari/537.36",
        model: "Desktop Win11",
        width: 1920,
        platform: "Windows",
        mobile: false,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Safari/537.36",
        model: "MacBook",
        width: 1440,
        platform: "macOS",
        mobile: false,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
    DeviceProfile {
        ua_template: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.{}.{} Safari/537.36",
        model: "Linux Desktop",
        width: 1920,
        platform: "Linux",
        mobile: false,
        brands: r#""Chromium";v="{}","Google Chrome";v="{}","Not=A?Brand";v="24""#,
    },
];

const ACCEPT_LANGUAGES: &[&str] = &[
    "es-AR,es;q=0.9,en;q=0.8",
    "es-ES,es;q=0.9,en;q=0.8",
    "es-MX,es;q=0.9,en-US;q=0.8,en;q=0.7",
    "en-US,en;q=0.9,es;q=0.8",
    "en-GB,en;q=0.9",
    "pt-BR,pt;q=0.9,en;q=0.8",
    "es-CL,es;q=0.9,en;q=0.8",
    "es-CO,es;q=0.9,en;q=0.8",
    "es-UY,es;q=0.9,en;q=0.8",
    "es-PE,es;q=0.9,en;q=0.8",
    "de-DE,de;q=0.9,en;q=0.8",
    "fr-FR,fr;q=0.9,en;q=0.8",
];

// ═══════════════════════════════════════════════════════════════════════════════
// DATA STRUCTURES
// ═══════════════════════════════════════════════════════════════════════════════
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RatedProxy {
    addr: String,
    speed_ms: u64,
    success_count: u32,
    fail_count: u32,
    last_used: u64,
    proxy_type: ProxyType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum ProxyType {
    Http,
    Socks5,
    Socks4,
}

impl RatedProxy {
    fn new(addr: String, speed_ms: u64, proxy_type: ProxyType) -> Self {
        Self {
            addr,
            speed_ms,
            success_count: 1,
            fail_count: 0,
            last_used: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            proxy_type,
        }
    }
    
    fn score(&self) -> u64 {
        // Higher score = better proxy
        // Fast speed, high success, low fail = good
        let speed_score = 10000_u64.saturating_sub(self.speed_ms);
        let success_score = self.success_count as u64 * 100;
        let fail_penalty = self.fail_count as u64 * 500;
        speed_score + success_score - fail_penalty.min(speed_score + success_score)
    }
}

impl Eq for RatedProxy {}
impl PartialEq for RatedProxy {
    fn eq(&self, other: &Self) -> bool {
        self.score() == other.score()
    }
}
impl Ord for RatedProxy {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.score().cmp(&other.score())
    }
}
impl PartialOrd for RatedProxy {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

struct Stats {
    impressions: AtomicU64,
    clicks: AtomicU64,
    errors: AtomicU64,
    unique_ips: AtomicU64,
    gold_saved: AtomicU64,
    validated: AtomicU64,
    retries: AtomicU64,
    // Ad network specific
    aads_hits: AtomicU64,
    monetag_hits: AtomicU64,
    popunder_triggers: AtomicU64,
    // Progress tracking
    validation_total: AtomicU64,
    validation_progress: AtomicU64,
}

impl Stats {
    fn new() -> Self {
        Self {
            impressions: AtomicU64::new(0),
            clicks: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            unique_ips: AtomicU64::new(0),
            gold_saved: AtomicU64::new(0),
            validated: AtomicU64::new(0),
            retries: AtomicU64::new(0),
            aads_hits: AtomicU64::new(0),
            monetag_hits: AtomicU64::new(0),
            popunder_triggers: AtomicU64::new(0),
            validation_total: AtomicU64::new(0),
            validation_progress: AtomicU64::new(0),
        }
    }
}

/// Proxy blacklist with TTL to avoid retrying dead proxies
struct ProxyBlacklist {
    entries: RwLock<HashMap<String, u64>>, // proxy addr -> expiry timestamp
    ttl_seconds: u64,
}

impl ProxyBlacklist {
    fn new(ttl_seconds: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl_seconds,
        }
    }
    
    /// Add a proxy to the blacklist
    async fn add(&self, addr: &str) {
        let expiry = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() + self.ttl_seconds;
        self.entries.write().await.insert(addr.to_string(), expiry);
    }
    
    /// Check if a proxy is blacklisted (and clean expired entries)
    async fn is_blacklisted(&self, addr: &str) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let mut entries = self.entries.write().await;
        
        // Remove expired entries
        entries.retain(|_, expiry| *expiry > now);
        
        // Check if this proxy is blacklisted
        entries.contains_key(addr)
    }
    
    /// Get blacklist size
    async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

struct AppState {
    proxies: RwLock<BinaryHeap<RatedProxy>>,
    burned_ips: RwLock<HashSet<String>>,
    daily_ips: RwLock<HashSet<String>>,
    blacklist: Arc<ProxyBlacklist>,
    stats: Arc<Stats>,
    target_url: String,
    tor_ports: Vec<u16>,
    config: RuntimeConfig,
    shutdown: AtomicBool,
}

struct RuntimeConfig {
    workers: usize,
    click_rate: f32,
    delay_ms: u64,
    timeout_sec: u64,
    refresh_mins: u64,
    quiet: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HUMAN BEHAVIOR SIMULATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate human-like delay with variance (not uniform distribution)
fn human_delay(base_ms: u64) -> Duration {
    let mut rng = rand::thread_rng();
    // Log-normal distribution approximation for human timing
    let variance: f64 = rng.gen_range(0.7..1.4);
    let jitter: i64 = rng.gen_range(-300..300);
    let adjusted = (base_ms as f64 * variance) as i64 + jitter;
    Duration::from_millis(adjusted.max(500) as u64)
}

/// Simulate page dwell time (how long a user stays on page)
fn simulate_dwell_time() -> Duration {
    let mut rng = rand::thread_rng();
    // Most users spend 5-20 seconds on ad pages
    let base_seconds = rng.gen_range(5..20);
    // Add some variance
    let variance: f64 = rng.gen_range(0.8..1.3);
    Duration::from_secs_f64(base_seconds as f64 * variance)
}

/// Simulate realistic scroll position for ad viewability
fn simulate_scroll_position(page_height: u32) -> u32 {
    let mut rng = rand::thread_rng();
    // 70% of time scroll to middle, 30% deeper
    if rng.gen_bool(0.7) {
        rng.gen_range(100..page_height / 2)
    } else {
        rng.gen_range(page_height / 2..page_height.saturating_sub(100))
    }
}

/// Generate organic click timing with human patterns
fn organic_click_delay() -> Duration {
    let mut rng = rand::thread_rng();
    // Humans take 200-800ms to click after seeing something
    let think_time = rng.gen_range(200..600);
    let reaction_time = rng.gen_range(80..200);
    Duration::from_millis(think_time + reaction_time)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AD NETWORK PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect ad network type from URL or HTML content
fn detect_ad_network(url: &str, body: &str) -> Option<&'static str> {
    let url_lower = url.to_lowercase();
    let body_lower = body.to_lowercase();
    
    // AADS (A-ADS crypto ads)
    if url_lower.contains("a-ads.com") || body_lower.contains("data-aa=") {
        return Some("aads");
    }
    
    // Monetag (popunders)
    if url_lower.contains("monetag") || body_lower.contains("surfrfrr") 
        || body_lower.contains("alwingulla") || body_lower.contains("niphaumeenses") {
        return Some("monetag");
    }
    
    // PropellerAds
    if url_lower.contains("propellerads") || url_lower.contains("propu.sh")
        || body_lower.contains("propellerads") {
        return Some("propellerads");
    }
    
    // Adsterra
    if url_lower.contains("adsterra") || body_lower.contains("adsterra.com") {
        return Some("adsterra");
    }
    
    // PopAds
    if url_lower.contains("popads.net") || body_lower.contains("popads.net") {
        return Some("popads");
    }
    
    // ExoClick
    if url_lower.contains("exoclick") || body_lower.contains("syndication.exoclick") {
        return Some("exoclick");
    }
    
    // HilltopAds
    if url_lower.contains("hilltopads") || body_lower.contains("hilltopads") {
        return Some("hilltopads");
    }
    
    // TrafficStars
    if url_lower.contains("trafficstars") || body_lower.contains("trafficstars") {
        return Some("trafficstars");
    }
    
    None
}

/// Calculate optimal click rate based on ad network
fn get_network_click_rate(network: Option<&str>, base_rate: f32) -> f32 {
    match network {
        Some("aads") => base_rate * 0.8,  // Conservative for crypto ads
        Some("monetag") => base_rate * 1.5,  // Boost for popunders
        Some("propellerads") => base_rate * 1.2,
        Some("popads") => base_rate * 1.4,
        Some("exoclick") => base_rate * 1.1,
        _ => base_rate,
    }.min(0.20)  // Cap at 20%
}

// ═══════════════════════════════════════════════════════════════════════════════
// TERMUX UTILITIES
// ═══════════════════════════════════════════════════════════════════════════════
fn is_termux() -> bool {
    std::env::var("PREFIX").map(|p| p.contains("com.termux")).unwrap_or(false)
        || std::path::Path::new("/data/data/com.termux").exists()
}

fn get_data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(format!("{}/.icecold", home));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn get_gold_proxies_path() -> PathBuf {
    get_data_dir().join(GOLD_PROXIES_FILE)
}

// ═══════════════════════════════════════════════════════════════════════════════
// PERSISTENT CACHE
// ═══════════════════════════════════════════════════════════════════════════════
async fn load_gold_proxies() -> BinaryHeap<RatedProxy> {
    let mut heap = BinaryHeap::new();
    let path = get_gold_proxies_path();
    
    if let Ok(file) = File::open(&path).await {
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(proxy) = serde_json::from_str::<RatedProxy>(&line) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                
                // Keep proxies from last 24 hours
                if now - proxy.last_used < 86400 {
                    heap.push(proxy);
                }
            }
        }
    }
    
    if !heap.is_empty() {
        println!("{}", format!("[✓] Loaded {} gold proxies", heap.len()).green());
    }
    heap
}

async fn save_gold_proxy(proxy: &RatedProxy) -> std::io::Result<()> {
    let path = get_gold_proxies_path();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    
    let json = serde_json::to_string(proxy)?;
    file.write_all(format!("{}\n", json).as_bytes()).await?;
    Ok(())
}

async fn save_stats(stats: &Stats) -> std::io::Result<()> {
    let path = get_data_dir().join("last_stats.json");
    let data = serde_json::json!({
        "impressions": stats.impressions.load(Ordering::Relaxed),
        "clicks": stats.clicks.load(Ordering::Relaxed),
        "errors": stats.errors.load(Ordering::Relaxed),
        "validated": stats.validated.load(Ordering::Relaxed),
        "gold_saved": stats.gold_saved.load(Ordering::Relaxed),
        "aads_hits": stats.aads_hits.load(Ordering::Relaxed),
        "monetag_hits": stats.monetag_hits.load(Ordering::Relaxed),
        "popunder_triggers": stats.popunder_triggers.load(Ordering::Relaxed),
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });
    
    tokio::fs::write(&path, serde_json::to_string_pretty(&data)?).await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROXY VALIDATION
// ═══════════════════════════════════════════════════════════════════════════════
async fn validate_proxy(addr: &str, proxy_type: ProxyType) -> Option<RatedProxy> {
    let test_urls = [
        "https://httpbin.org/ip",
        "https://api.ipify.org?format=json",
        "https://ifconfig.me/ip",
    ];
    
    let proxy_url = match proxy_type {
        ProxyType::Http => format!("http://{}", addr),
        ProxyType::Socks5 => format!("socks5://{}", addr),
        ProxyType::Socks4 => format!("socks4://{}", addr),
    };
    
    let proxy = reqwest::Proxy::all(&proxy_url).ok()?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_millis(VALIDATION_TIMEOUT_MS))
        .build()
        .ok()?;
    
    let start = Instant::now();
    
    for test_url in test_urls {
        match client.get(test_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let speed_ms = start.elapsed().as_millis() as u64;
                
                if speed_ms < MIN_GOLD_SPEED_MS {
                    return Some(RatedProxy::new(addr.to_string(), speed_ms, proxy_type));
                }
                return None;
            }
            _ => continue,
        }
    }
    
    None
}

async fn validate_proxies_batch(proxies: Vec<(String, ProxyType)>) -> Vec<RatedProxy> {
    let semaphore = Arc::new(Semaphore::new(50)); // Limit concurrent validations
    let mut handles = Vec::new();
    
    for (proxy, ptype) in proxies {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        handles.push(tokio::spawn(async move {
            let result = validate_proxy(&proxy, ptype).await;
            drop(permit);
            result
        }));
    }
    
    let mut validated = Vec::new();
    for handle in handles {
        if let Ok(Some(proxy)) = handle.await {
            validated.push(proxy);
        }
    }
    
    validated
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOR MANAGEMENT (Termux Compatible)
// ═══════════════════════════════════════════════════════════════════════════════
async fn check_tor_available() -> bool {
    // Check if tor is installed
    let result = tokio::process::Command::new("which")
        .arg("tor")
        .output()
        .await;
    
    if result.is_err() || !result.unwrap().status.success() {
        // Try Termux path
        if std::path::Path::new("/data/data/com.termux/files/usr/bin/tor").exists() {
            return true;
        }
        return false;
    }
    true
}

async fn stop_existing_tor() {
    // Use killall on Termux instead of pkill
    let commands = if is_termux() {
        vec!["killall", "pkill"]
    } else {
        vec!["pkill", "killall"]
    };
    
    for cmd in commands {
        let _ = tokio::process::Command::new(cmd)
            .arg("tor")
            .output()
            .await;
    }
    
    sleep(Duration::from_millis(500)).await;
}

async fn start_tor_pool(instances: usize) -> Result<Vec<u16>> {
    if !check_tor_available().await {
        println!("{}", "[!] Tor not found, running without Tor".yellow());
        return Ok(vec![]);
    }
    
    stop_existing_tor().await;
    
    println!("{}", format!("[*] Starting {} Tor nodes...", instances).yellow());
    
    let data_dir = get_data_dir().join("tor_pool");
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir)?;
    
    let mut ports = Vec::new();
    
    for i in 0..instances {
        let socks_port = BASE_SOCKS_PORT + i as u16;
        let ctrl_port = 9250 + i as u16;
        let node_dir = data_dir.join(format!("node_{}", i));
        std::fs::create_dir_all(&node_dir)?;
        
        let result = tokio::process::Command::new("tor")
            .args([
                "--SocksPort", &socks_port.to_string(),
                "--ControlPort", &ctrl_port.to_string(),
                "--DataDirectory", node_dir.to_str().unwrap(),
                "--ExitNodes", "{us},{ca},{gb},{de},{fr},{nl}",
                "--NewCircuitPeriod", "150",
                "--MaxCircuitDirtiness", "300",
                "--quiet",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        
        if result.is_ok() {
            ports.push(socks_port);
        }
    }
    
    if ports.is_empty() {
        println!("{}", "[!] Failed to start Tor nodes".red());
        return Ok(vec![]);
    }
    
    // Wait for Tor to bootstrap
    sleep(Duration::from_secs(15)).await;
    println!("{}", format!("[✓] Tor active with {} nodes", ports.len()).green());
    
    // Start circuit rotation task
    let rotation_ports: Vec<u16> = (0..instances).map(|i| 9250 + i as u16).collect();
    tokio::spawn(async move {
        let mut idx = 0;
        loop {
            sleep(Duration::from_secs(20)).await;
            let port = rotation_ports[idx % rotation_ports.len()];
            if let Ok(mut stream) = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
                use tokio::io::AsyncWriteExt;
                let _ = stream.write_all(b"AUTHENTICATE \"\"\r\nSIGNAL NEWNYM\r\n").await;
            }
            idx = (idx + 1) % rotation_ports.len();
        }
    });
    
    Ok(ports)
}

// ═══════════════════════════════════════════════════════════════════════════════
// FETCH PROXIES
// ═══════════════════════════════════════════════════════════════════════════════
async fn fetch_and_validate_proxies(stats: Arc<Stats>, quiet: bool) -> BinaryHeap<RatedProxy> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .unwrap();
    
    if !quiet {
        println!("{}", "[*] Downloading proxies...".yellow());
    }
    
    let mut handles = Vec::new();
    
    for &url in PROXY_SOURCES {
        let client = client.clone();
        let is_socks5 = url.contains("socks5");
        let is_socks4 = url.contains("socks4");
        
        handles.push(tokio::spawn(async move {
            match client.get(url).send().await {
                Ok(resp) => {
                    if let Ok(text) = resp.text().await {
                        let ptype = if is_socks5 {
                            ProxyType::Socks5
                        } else if is_socks4 {
                            ProxyType::Socks4
                        } else {
                            ProxyType::Http
                        };
                        PROXY_REGEX.find_iter(&text)
                            .map(|m| (m.as_str().to_string(), ptype.clone()))
                            .collect::<Vec<_>>()
                    } else { 
                        Vec::new() 
                    }
                }
                Err(_) => Vec::new(),
            }
        }));
    }
    
    let mut all_proxies = Vec::new();
    for handle in handles {
        if let Ok(proxies) = handle.await {
            all_proxies.extend(proxies);
        }
    }
    
    // Deduplicate by address
    let mut seen = HashSet::new();
    let unique: Vec<(String, ProxyType)> = all_proxies
        .into_iter()
        .filter(|(addr, _)| seen.insert(addr.clone()))
        .collect();
    
    if !quiet {
        println!("{}", format!("[*] {} unique. Validating...", unique.len()).yellow());
    }
    
    let mut validated_heap = BinaryHeap::new();
    
    for chunk in unique.chunks(VALIDATION_BATCH_SIZE) {
        let validated = validate_proxies_batch(chunk.to_vec()).await;
        
        for proxy in validated {
            stats.validated.fetch_add(1, Ordering::Relaxed);
            
            if proxy.speed_ms < MIN_GOLD_SPEED_MS {
                let _ = save_gold_proxy(&proxy).await;
                stats.gold_saved.fetch_add(1, Ordering::Relaxed);
            }
            
            validated_heap.push(proxy);
            
            if validated_heap.len() >= MAX_PROXIES_CACHE {
                break;
            }
        }
    }
    
    if !quiet {
        println!("{}", format!("[✓] {} validated", validated_heap.len()).green());
    }
    validated_heap
}

/// Fetch and validate proxies with configurable thread count
async fn fetch_and_validate_proxies_threaded(stats: Arc<Stats>, quiet: bool, threads: usize) -> BinaryHeap<RatedProxy> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .build()
        .unwrap();
    
    if !quiet {
        println!("{}", format!("[*] Downloading proxies (using {} validation threads)...", threads).yellow());
    }
    
    let mut handles = Vec::new();
    
    for &url in PROXY_SOURCES {
        let client = client.clone();
        let is_socks5 = url.contains("socks5");
        let is_socks4 = url.contains("socks4");
        
        handles.push(tokio::spawn(async move {
            match client.get(url).send().await {
                Ok(resp) => {
                    if let Ok(text) = resp.text().await {
                        let re = Regex::new(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d{2,5}").unwrap();
                        let ptype = if is_socks5 {
                            ProxyType::Socks5
                        } else if is_socks4 {
                            ProxyType::Socks4
                        } else {
                            ProxyType::Http
                        };
                        re.find_iter(&text)
                            .map(|m| (m.as_str().to_string(), ptype.clone()))
                            .collect::<Vec<_>>()
                    } else { 
                        Vec::new() 
                    }
                }
                Err(_) => Vec::new(),
            }
        }));
    }
    
    let mut all_proxies = Vec::new();
    for handle in handles {
        if let Ok(proxies) = handle.await {
            all_proxies.extend(proxies);
        }
    }
    
    // Deduplicate by address
    let mut seen = HashSet::new();
    let unique: Vec<(String, ProxyType)> = all_proxies
        .into_iter()
        .filter(|(addr, _)| seen.insert(addr.clone()))
        .collect();
    
    if !quiet {
        println!("{}", format!("[*] {} unique. Validating with {} threads...", unique.len(), threads).yellow());
    }
    
    // Create progress bar
    let pb = if !quiet {
        let bar = ProgressBar::new(unique.len() as u64);
        bar.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("█▓░"));
        Some(bar)
    } else {
        None
    };
    
    let mut validated_heap = BinaryHeap::new();
    let semaphore = Arc::new(Semaphore::new(threads));
    let validated_count = Arc::new(AtomicU64::new(0));
    
    for chunk in unique.chunks(VALIDATION_BATCH_SIZE) {
        let mut chunk_handles = Vec::new();
        
        for (proxy, ptype) in chunk.to_vec() {
            let sem = semaphore.clone();
            let pb_clone = pb.clone();
            let count = validated_count.clone();
            chunk_handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let result = validate_proxy(&proxy, ptype).await;
                count.fetch_add(1, Ordering::Relaxed);
                if let Some(ref bar) = pb_clone {
                    bar.inc(1);
                }
                result
            }));
        }
        
        for handle in chunk_handles {
            if let Ok(Some(proxy)) = handle.await {
                stats.validated.fetch_add(1, Ordering::Relaxed);
                
                if proxy.speed_ms < MIN_GOLD_SPEED_MS {
                    let _ = save_gold_proxy(&proxy).await;
                    stats.gold_saved.fetch_add(1, Ordering::Relaxed);
                }
                
                validated_heap.push(proxy);
                
                if validated_heap.len() >= MAX_PROXIES_CACHE {
                    break;
                }
            }
        }
    }
    
    if !quiet {
        println!("{}", format!("[✓] {} validated", validated_heap.len()).green());
    }
    validated_heap
}

// ═══════════════════════════════════════════════════════════════════════════════
// REQUEST WITH RETRY
// ═══════════════════════════════════════════════════════════════════════════════
async fn request_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
    stats: &Arc<Stats>,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut last_error = None;
    
    for attempt in 0..MAX_RETRIES {
        match client.get(url).headers(headers.clone()).send().await {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES - 1 {
                    stats.retries.fetch_add(1, Ordering::Relaxed);
                    // Exponential backoff: 100ms, 200ms, 400ms
                    let delay = 100 * (2_u64.pow(attempt));
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }
    
    Err(last_error.unwrap())
}

// ═══════════════════════════════════════════════════════════════════════════════
// WORKER
// ═══════════════════════════════════════════════════════════════════════════════
async fn worker(id: usize, state: Arc<AppState>, semaphore: Arc<Semaphore>) {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::from_entropy();
    
    loop {
        if state.shutdown.load(Ordering::Relaxed) {
            break;
        }
        
        let _permit = semaphore.acquire().await.unwrap();
        
        // Human-like delay with variance
        let base_delay = state.config.delay_ms;
        let variance = (base_delay as f64 * 0.5) as u64;
        let sleep_ms = base_delay + rng.gen_range(0..variance);
        sleep(Duration::from_millis(sleep_ms)).await;
        
        // 60% Tor, 40% Proxy (when Tor available)
        let use_tor = !state.tor_ports.is_empty() && rng.gen_bool(0.60);
        
        let (client, proxy_addr) = if use_tor {
            let port = state.tor_ports[rng.gen_range(0..state.tor_ports.len())];
            let proxy = match reqwest::Proxy::all(format!("socks5://127.0.0.1:{}", port)) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let c = match reqwest::Client::builder()
                .proxy(proxy)
                .timeout(Duration::from_secs(state.config.timeout_sec))
                .pool_max_idle_per_host(10)
                .build() {
                    Ok(c) => c,
                    Err(_) => continue,
            };
            (c, format!("tor:{}", port))
        } else {
            let proxy_info = {
                let mut proxies = state.proxies.write().await;
                proxies.pop()
            };
            
            match proxy_info {
                Some(mut proxy) => {
                    if state.burned_ips.read().await.contains(&proxy.addr) {
                        continue;
                    }
                    
                    let proxy_url = match proxy.proxy_type {
                        ProxyType::Http => format!("http://{}", &proxy.addr),
                        ProxyType::Socks5 => format!("socks5://{}", &proxy.addr),
                        ProxyType::Socks4 => format!("socks4://{}", &proxy.addr),
                    };
                    
                    let proxy_obj = match reqwest::Proxy::all(&proxy_url) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    
                    let c = match reqwest::Client::builder()
                        .proxy(proxy_obj)
                        .timeout(Duration::from_secs(state.config.timeout_sec))
                        .pool_max_idle_per_host(5)
                        .build() {
                            Ok(c) => c,
                            Err(_) => continue,
                    };
                    
                    let addr = proxy.addr.clone();
                    proxy.success_count += 1;
                    proxy.last_used = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    state.proxies.write().await.push(proxy);
                    
                    (c, addr)
                }
                None => continue,
            }
        };
        
        // Generate realistic fingerprint
        let profile = &PROFILES[rng.gen_range(0..PROFILES.len())];
        let chrome_major: u32 = rng.gen_range(120..127);
        let chrome_build: u32 = rng.gen_range(1000..6000);
        let chrome_patch: u32 = rng.gen_range(1..200);
        
        let ua = profile.ua_template
            .replacen("{}", &chrome_major.to_string(), 1)
            .replacen("{}", &chrome_build.to_string(), 1)
            .replacen("{}", &chrome_patch.to_string(), 1);
        
        let brands = profile.brands
            .replacen("{}", &chrome_major.to_string(), 1)
            .replacen("{}", &chrome_major.to_string(), 1);
        
        let referer = if rng.gen_bool(0.6) {
            SEARCH_ENGINES[rng.gen_range(0..SEARCH_ENGINES.len())]
        } else {
            INTERNAL_PAGES[rng.gen_range(0..INTERNAL_PAGES.len())]
        };
        
        let accept_lang = ACCEPT_LANGUAGES[rng.gen_range(0..ACCEPT_LANGUAGES.len())];
        
        // Build headers with Client Hints
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", ua.parse().unwrap());
        headers.insert("Referer", referer.parse().unwrap());
        headers.insert("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8".parse().unwrap());
        headers.insert("Accept-Encoding", "gzip, deflate, br".parse().unwrap());
        headers.insert("Accept-Language", accept_lang.parse().unwrap());
        headers.insert("Cache-Control", "no-cache".parse().unwrap());
        headers.insert("Pragma", "no-cache".parse().unwrap());
        
        // Client Hints (modern browsers)
        if profile.platform != "iOS" {
            headers.insert("Sec-CH-UA", brands.parse().unwrap());
            headers.insert("Sec-CH-UA-Mobile", if profile.mobile { "?1" } else { "?0" }.parse().unwrap());
            headers.insert("Sec-CH-UA-Platform", format!("\"{}\"", profile.platform).parse().unwrap());
            headers.insert("Sec-Fetch-Dest", "document".parse().unwrap());
            headers.insert("Sec-Fetch-Mode", "navigate".parse().unwrap());
            headers.insert("Sec-Fetch-Site", "cross-site".parse().unwrap());
            headers.insert("Sec-Fetch-User", "?1".parse().unwrap());
        }
        
        match request_with_retry(&client, &state.target_url, headers.clone(), &state.stats).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.bytes().await.unwrap_or_default();
                let body_str = String::from_utf8_lossy(&body);
                
                if (200..400).contains(&status) {
                    state.stats.impressions.fetch_add(1, Ordering::Relaxed);
                    
                    // Track ad network type
                    let target_lower = state.target_url.to_lowercase();
                    if target_lower.contains("a-ads") || target_lower.contains("aads") {
                        state.stats.aads_hits.fetch_add(1, Ordering::Relaxed);
                    } else if target_lower.contains("monetag") || target_lower.contains("surfrfrr") 
                           || target_lower.contains("alwingulla") || target_lower.contains("niphaumeenses") {
                        state.stats.monetag_hits.fetch_add(1, Ordering::Relaxed);
                    }
                    
                    // Check for popunder triggers in response
                    if body_str.contains("onclick") || body_str.contains("popunder") 
                       || body_str.contains("window.open") {
                        state.stats.popunder_triggers.fetch_add(1, Ordering::Relaxed);
                    }
                    
                    // Log only from worker 0 to reduce spam
                    if id == 0 && !state.config.quiet {
                        println!("{} {} via {}", "[HIT]".green(), status, proxy_addr.dimmed());
                    }
                    
                    // Simulate human reading time (log-normal distribution)
                    let read_time = (rng.gen::<f32>().ln().abs() * 2500.0).min(5000.0) as u64;
                    sleep(Duration::from_millis(read_time)).await;
                    
                    // Click with configured rate (higher for Monetag popunders)
                    let effective_click_rate = if target_lower.contains("monetag") {
                        (state.config.click_rate * 1.5).min(0.15) // Boost for Monetag
                    } else {
                        state.config.click_rate
                    };
                    
                    if rng.gen_bool(effective_click_rate as f64) {
                        sleep(Duration::from_millis(rng.gen_range(200..800))).await;
                        let _ = request_with_retry(&client, &state.target_url, headers, &state.stats).await;
                        state.stats.clicks.fetch_add(1, Ordering::Relaxed);
                    }
                } else {
                    state.stats.errors.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(_) => {
                state.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MONITOR
// ═══════════════════════════════════════════════════════════════════════════════
async fn monitor(state: Arc<AppState>, start: Instant) {
    let is_term = is_termux();
    
    loop {
        if state.shutdown.load(Ordering::Relaxed) {
            break;
        }
        
        sleep(Duration::from_secs(2)).await;
        
        let elapsed = start.elapsed();
        let secs = elapsed.as_secs_f64();
        let mins = elapsed.as_secs() / 60;
        let hrs = mins / 60;
        
        let i = state.stats.impressions.load(Ordering::Relaxed);
        let c = state.stats.clicks.load(Ordering::Relaxed);
        let e = state.stats.errors.load(Ordering::Relaxed);
        let v = state.stats.validated.load(Ordering::Relaxed);
        let g = state.stats.gold_saved.load(Ordering::Relaxed);
        let r = state.stats.retries.load(Ordering::Relaxed);
        
        // Ad network stats
        let aads = state.stats.aads_hits.load(Ordering::Relaxed);
        let monetag = state.stats.monetag_hits.load(Ordering::Relaxed);
        let pop = state.stats.popunder_triggers.load(Ordering::Relaxed);
        
        let rpm = if secs > 0.0 { (i as f64 / secs) * 60.0 } else { 0.0 };
        let ctr = if i > 0 { (c as f64 / i as f64) * 100.0 } else { 0.0 };
        let err_rate = if i + e > 0 { (e as f64 / (i + e) as f64) * 100.0 } else { 0.0 };
        
        let proxy_count = state.proxies.read().await.len();
        let tor_status = if state.tor_ports.is_empty() { "OFF" } else { "ON" };
        
        // Clear screen (works on Termux)
        print!("\x1b[H\x1b[2J");
        
        let title = if is_term { "ICE COLD v36 [TERMUX]" } else { "ICE COLD v36 [RUST]" };
        
        println!("{}", format!("╔════════════════ {} ════════════════╗", title).magenta());
        println!("{}", format!("║ Workers: {:>4} │ Proxies: {:>5} │ Tor: {:>3}   ║", 
            state.config.workers, proxy_count, tor_status).magenta());
        println!("{}", "╠══════════════════════════════════════════════════╣".magenta());
        println!("{}", format!("║  IMP: {:>8} │ CLK: {:>6} │ CTR: {:>5.2}%    ║", i, c, ctr).cyan());
        println!("{}", format!("║  ERR: {:>8} │ RPM: {:>6.0} │ ERR%: {:>4.1}%    ║", e, rpm, err_rate).cyan());
        println!("{}", "╠══════════════════════════════════════════════════╣".magenta());
        println!("{}", format!("║  AADS: {:>6} │ MNTG: {:>5} │ POP: {:>6}   ║", aads, monetag, pop).green());
        println!("{}", "╠══════════════════════════════════════════════════╣".magenta());
        println!("{}", format!("║  VAL: {:>8} │ GOLD: {:>5} │ RETRY: {:>5}  ║", v, g, r).yellow());
        println!("{}", format!("║  Runtime: {:02}h {:02}m {:02}s                          ║", 
            hrs, mins % 60, elapsed.as_secs() % 60).yellow());
        println!("{}", "╚══════════════════════════════════════════════════╝".magenta());
        
        // Save stats periodically
        if elapsed.as_secs() % 60 == 0 {
            let _ = save_stats(&state.stats).await;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AD NETWORK URL EXTRACTION (AADS, Monetag, etc.)
// ═══════════════════════════════════════════════════════════════════════════════
fn extract_url_from_html(html: &str) -> Option<String> {
    // AADS patterns (iframe-based crypto ads)
    let aads_patterns = [
        r#"(?:https?:)?//ad\.a-ads\.com/\d+"#,
        r#"(?:https?:)?//[^"'\s]+a-ads\.com[^"'\s]*"#,
        r#"data-aa=["'](\d+)["']"#,
    ];
    
    // Monetag patterns (JS-based popunder/banners)
    let monetag_patterns = [
        r#"(?:https?:)?//[^"'\s]*monetag[^"'\s]*\.js"#,
        r#"(?:https?:)?//[^"'\s]*alwingulla[^"'\s]*"#,
        r#"(?:https?:)?//[^"'\s]*surfrfrr[^"'\s]*"#,
        r#"(?:https?:)?//[^"'\s]*niphaumeenses[^"'\s]*"#,
        r#"data-cfasync[^>]+src=["']([^"']+)["']"#,
    ];
    
    // Generic ad network patterns  
    let generic_patterns = [
        r#"src\s*=\s*["']((?:https?:)?//[^"']+)["']"#,
        r#"href\s*=\s*["'](https?://[^"']+)["']"#,
        r#"data-src\s*=\s*["']((?:https?:)?//[^"']+)["']"#,
        r#"(https?://[^\s<>"']+)"#,
    ];
    
    // Try AADS first (check for data-aa attribute)
    if let Ok(re) = Regex::new(r#"data-aa=["']?(\d+)["']?"#) {
        if let Some(caps) = re.captures(html) {
            if let Some(m) = caps.get(1) {
                return Some(format!("https://ad.a-ads.com/{}", m.as_str()));
            }
        }
    }
    
    // Try all AADS patterns
    for pattern in aads_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(m) = re.find(html) {
                let mut url = m.as_str().to_string();
                if url.starts_with("//") {
                    url = format!("https:{}", url);
                }
                return Some(url.replace("&amp;", "&"));
            }
        }
    }
    
    // Try Monetag patterns
    for pattern in monetag_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(html) {
                if let Some(m) = caps.get(1).or_else(|| caps.get(0)) {
                    let mut url = m.as_str().to_string();
                    if url.starts_with("//") {
                        url = format!("https:{}", url);
                    }
                    return Some(url.replace("&amp;", "&"));
                }
            }
        }
    }
    
    // Try generic patterns
    for pattern in generic_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(html) {
                if let Some(m) = caps.get(1) {
                    let mut url = m.as_str().to_string();
                    if url.starts_with("//") {
                        url = format!("https:{}", url);
                    }
                    return Some(url.replace("&amp;", "&"));
                }
            }
        }
    }
    
    None
}

/// Extract ALL ad URLs from HTML (for multi-ad support)
fn extract_all_ad_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    
    let patterns = [
        r#"(?:https?:)?//ad\.a-ads\.com/\d+[^"'\s]*"#,
        r#"(?:https?:)?//[^"'\s]*monetag[^"'\s]*"#,
        r#"(?:https?:)?//[^"'\s]*surfrfrr[^"'\s]*"#,
        r#"src\s*=\s*["']((?:https?:)?//[^"']+)["']"#,
    ];
    
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            for caps in re.captures_iter(html) {
                let url_match = caps.get(1).or_else(|| caps.get(0));
                if let Some(m) = url_match {
                    let mut url = m.as_str().to_string();
                    if url.starts_with("//") {
                        url = format!("https:{}", url);
                    }
                    url = url.replace("&amp;", "&");
                    if !seen.contains(&url) && (url.contains("ad") || url.contains("monetag") || url.contains(".js")) {
                        seen.insert(url.clone());
                        urls.push(url);
                    }
                }
            }
        }
    }
    
    // Check for AADS data-aa attribute
    if let Ok(re) = Regex::new(r#"data-aa=["']?(\d+)["']?"#) {
        for caps in re.captures_iter(html) {
            if let Some(m) = caps.get(1) {
                let url = format!("https://ad.a-ads.com/{}", m.as_str());
                if !seen.contains(&url) {
                    seen.insert(url.clone());
                    urls.push(url);
                }
            }
        }
    }
    
    urls
}

async fn get_target_url(args: &Args, config: &Config) -> Result<String> {
    // Priority: CLI arg > Config > Interactive input
    if let Some(url) = &args.url {
        if url.starts_with("http") {
            return Ok(url.clone());
        }
        // Maybe it's HTML with embedded URL
        if let Some(extracted) = extract_url_from_html(url) {
            return Ok(extracted);
        }
    }
    
    if let Some(url) = &config.target_url {
        return Ok(url.clone());
    }
    
    // Interactive mode
    println!("{}", "\nPaste HTML/URL and press Enter twice:".yellow());
    
    let mut input = String::new();
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();
    
    let mut empty_count = 0;
    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() {
            empty_count += 1;
            if empty_count >= 2 {
                break;
            }
        } else {
            empty_count = 0;
            input.push_str(&line);
            input.push('\n');
        }
    }
    
    // Try to find URL in input
    if input.starts_with("http") {
        let url = input.lines().next().unwrap_or("").trim().to_string();
        if !url.is_empty() {
            return Ok(url);
        }
    }
    
    extract_url_from_html(&input)
        .context("No URL found in input. Use --url or paste valid HTML with iframe.")
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAIN
// ═══════════════════════════════════════════════════════════════════════════════
#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();
    
    // Load config file
    let config = load_config(args.config.clone()).await;
    
    // Clear screen
    print!("\x1b[H\x1b[2J");
    
    let env_type = if is_termux() { "TERMUX" } else { "RUST" };
    println!("{}", format!("=== ICE COLD v36 - {} EDITION ===\n", env_type).magenta().bold());
    
    // Get target URL
    let target_url = get_target_url(&args, &config).await?;
    println!("{}", format!("[✓] Target: {}", target_url).green());
    
    // Interactive: Ask for workers count if not specified via CLI
    let workers = if args.workers == 50 && config.workers.is_none() {
        // Default value, ask user
        println!("{}", "\n¿Cuántos workers/hilos quieres usar? [default: 50]:".cyan());
        let mut input = String::new();
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();
        
        if let Ok(Some(line)) = lines.next_line().await {
            line.trim().parse::<usize>().unwrap_or(50)
        } else {
            50
        }
    } else {
        config.workers.unwrap_or(args.workers)
    };
    println!("{}", format!("[✓] Workers: {}", workers).green());
    
    // Interactive: Ask for validation threads
    println!("{}", "\n¿Cuántos hilos para validar proxies? [default: 50]:".cyan());
    let validation_threads = {
        let mut input = String::new();
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();
        
        if let Ok(Some(line)) = lines.next_line().await {
            let val = line.trim().parse::<usize>().unwrap_or(50);
            val.max(10).min(200) // Clamp between 10 and 200
        } else {
            50
        }
    };
    println!("{}", format!("[✓] Validation threads: {}", validation_threads).green());
    
    // Start Tor if enabled
    let tor_instances = config.tor_instances.unwrap_or(args.tor_instances);
    let tor_ports = if args.no_tor || config.no_tor.unwrap_or(false) {
        println!("{}", "[*] Tor disabled by config".yellow());
        vec![]
    } else {
        start_tor_pool(tor_instances).await?
    };
    
    // Initialize stats
    let stats = Arc::new(Stats::new());
    
    // Load existing gold proxies and fetch new ones
    let mut gold_proxies = load_gold_proxies().await;
    let new_proxies = fetch_and_validate_proxies_threaded(stats.clone(), args.quiet, validation_threads).await;
    
    for proxy in new_proxies {
        gold_proxies.push(proxy);
    }
    
    // Runtime config
    let runtime_config = RuntimeConfig {
        workers,
        click_rate: config.click_rate.unwrap_or(args.click_rate),
        delay_ms: config.delay_ms.unwrap_or(args.delay),
        timeout_sec: config.timeout_sec.unwrap_or(args.timeout),
        refresh_mins: config.refresh_mins.unwrap_or(args.refresh),
        quiet: args.quiet,
    };
    
    // Create proxy blacklist (5 min TTL)
    let blacklist = Arc::new(ProxyBlacklist::new(300));
    
    // Create app state
    let state = Arc::new(AppState {
        proxies: RwLock::new(gold_proxies),
        burned_ips: RwLock::new(HashSet::new()),
        daily_ips: RwLock::new(HashSet::new()),
        blacklist: blacklist.clone(),
        stats: stats.clone(),
        target_url,
        tor_ports,
        config: runtime_config,
        shutdown: AtomicBool::new(false),
    });
    
    // Start proxy refresh task
    let state_refresh = state.clone();
    let stats_refresh = stats.clone();
    let refresh_mins = state.config.refresh_mins;
    let quiet = state.config.quiet;
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(refresh_mins * 60)).await;
            if state_refresh.shutdown.load(Ordering::Relaxed) {
                break;
            }
            let new = fetch_and_validate_proxies(stats_refresh.clone(), quiet).await;
            let mut proxies = state_refresh.proxies.write().await;
            for proxy in new {
                proxies.push(proxy);
            }
        }
    });
    
    // Start monitor
    let state_monitor = state.clone();
    tokio::spawn(monitor(state_monitor, Instant::now()));
    
    // Start workers
    let semaphore = Arc::new(Semaphore::new(workers));
    let mut handles = Vec::new();
    
    for i in 0..workers {
        let state = state.clone();
        let sem = semaphore.clone();
        handles.push(tokio::spawn(worker(i, state, sem)));
    }
    
    // Wait for Ctrl+C
    println!("{}", format!("\n[*] Starting {} workers. Press Ctrl+C to stop.\n", workers).cyan());
    
    tokio::signal::ctrl_c().await?;
    
    // Graceful shutdown
    println!("\n{}", "[*] Shutting down...".yellow());
    state.shutdown.store(true, Ordering::Relaxed);
    
    // Save final stats
    let _ = save_stats(&stats).await;
    
    // Print final stats
    let i = stats.impressions.load(Ordering::Relaxed);
    let c = stats.clicks.load(Ordering::Relaxed);
    let e = stats.errors.load(Ordering::Relaxed);
    let ctr = if i > 0 { (c as f64 / i as f64) * 100.0 } else { 0.0 };
    
    println!("\n{}", "═══════════ FINAL STATS ═══════════".magenta());
    println!("  Impressions: {}", i.to_string().green());
    println!("  Clicks: {} ({:.2}%)", c.to_string().cyan(), ctr);
    println!("  Errors: {}", e.to_string().red());
    println!("{}", "════════════════════════════════════".magenta());
    
    // Stop Tor processes
    stop_existing_tor().await;
    
    Ok(())
}
