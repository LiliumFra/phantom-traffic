// ═══════════════════════════════════════════════════════════════════════════════
// ICE COLD v38 - A-ADS OPTIMIZED EDITION
// Ultra-optimized traffic bot for A-Ads unique impressions
// IP Freshness Tracking | Aggressive Tor Rotation | High-CPM Geotargeting
// ═══════════════════════════════════════════════════════════════════════════════

use std::sync::atomic::{AtomicU64, AtomicI64, AtomicBool, Ordering};
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
use chrono::Utc;

// Progress bar styles
use indicatif::{ProgressBar, ProgressStyle};

// ═══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════
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
// ANTI-DETECTION: SEC-FETCH HEADERS & REFERER CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

/// Realistic referer chains (simulates organic discovery)
const GOOGLE_REFERERS: &[&str] = &[
    "https://www.google.com/",
    "https://www.google.com/search?q=dolar+blue+argentina",
    "https://www.google.com/search?q=cotizacion+dolar+hoy",
    "https://www.google.com/search?q=crypto+usdt+p2p",
    "https://www.google.com.ar/",
];

const SOCIAL_REFERERS: &[&str] = &[
    "https://www.facebook.com/",
    "https://t.co/",
    "https://www.reddit.com/",
    "https://www.youtube.com/",
];

const DIRECT_REFERERS: &[&str] = &[
    "",  // Direct visit (no referer)
    "android-app://com.android.chrome/",
];

/// Select a realistic referer based on probability
fn get_organic_referer() -> &'static str {
    let mut rng = rand::thread_rng();
    let roll: f32 = rng.gen();
    
    if roll < 0.45 {
        // 45% from Google search
        GOOGLE_REFERERS[rng.gen_range(0..GOOGLE_REFERERS.len())]
    } else if roll < 0.65 {
        // 20% from social media
        SOCIAL_REFERERS[rng.gen_range(0..SOCIAL_REFERERS.len())]
    } else {
        // 35% direct visit
        DIRECT_REFERERS[rng.gen_range(0..DIRECT_REFERERS.len())]
    }
}

/// Get Sec-Fetch headers for Chrome (anti-detection)
fn get_sec_fetch_headers_str(is_iframe: bool) -> [(&'static str, &'static str); 4] {
    if is_iframe {
        [
            ("Sec-Fetch-Mode", "navigate"),
            ("Sec-Fetch-Site", "cross-site"),
            ("Sec-Fetch-Dest", "iframe"),
            ("Sec-Fetch-User", ""),
        ]
    } else {
        [
            ("Sec-Fetch-Mode", "navigate"),
            ("Sec-Fetch-Site", "none"),
            ("Sec-Fetch-Dest", "document"),
            ("Sec-Fetch-User", "?1"),
        ]
    }
}

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
    // Phase 3: Domain-specific success/failure
    domain_stats: HashMap<String, (u32, u32)>, // domain -> (success, fail)
    // Phase 3: Cached country code for CPM scoring
    #[serde(default)]
    country: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
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
            domain_stats: HashMap::new(),
            country: None,
        }
    }
    
    fn score(&self) -> u64 {
        // Higher score = better proxy
        let speed_score = 10000_u64.saturating_sub(self.speed_ms);
        let success_score = self.success_count as u64 * 100;
        let fail_penalty = self.fail_count as u64 * 500;
        speed_score + success_score - fail_penalty.min(speed_score + success_score)
    }

    fn domain_score(&self, domain: &str) -> u64 {
        let base = self.score();
        if let Some((s, f)) = self.domain_stats.get(domain) {
            let domain_success = *s as u64 * 200;
            let domain_fail = *f as u64 * 1000;
            (base + domain_success).saturating_sub(domain_fail)
        } else {
            base
        }
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
    // A-Ads unique impression tracking (CRITICAL for earnings)
    aads_unique_impressions: AtomicU64,
    aads_non_unique_impressions: AtomicU64,
    // Progress tracking
    validation_total: AtomicU64,
    validation_progress: AtomicU64,
    // Phase 3: Auto-stop tracking (timestamp when low rate started)
    low_unique_rate_since: AtomicI64,
    // Phase 3: Rate limit tracking
    rate_limited_count: AtomicU64,
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
            aads_unique_impressions: AtomicU64::new(0),
            aads_non_unique_impressions: AtomicU64::new(0),
            validation_total: AtomicU64::new(0),
            validation_progress: AtomicU64::new(0),
            low_unique_rate_since: AtomicI64::new(0),
            rate_limited_count: AtomicU64::new(0),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RATE LIMITER WITH EXPONENTIAL BACKOFF (Phase 3)
// ═══════════════════════════════════════════════════════════════════════════════
struct RateLimiter {
    consecutive_errors: RwLock<HashMap<String, u32>>,  // domain -> error count
    backoff_until: RwLock<HashMap<String, i64>>,       // domain -> timestamp
    max_backoff_secs: u64,
    base_delay_ms: u64,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            consecutive_errors: RwLock::new(HashMap::new()),
            backoff_until: RwLock::new(HashMap::new()),
            max_backoff_secs: 300, // Max 5 minutes backoff
            base_delay_ms: 1000,
        }
    }
    
    /// Check if we should wait before making a request to this domain
    async fn should_wait(&self, domain: &str) -> Option<Duration> {
        let now = Utc::now().timestamp();
        let backoff = self.backoff_until.read().await;
        
        if let Some(&until) = backoff.get(domain) {
            if now < until {
                return Some(Duration::from_secs((until - now) as u64));
            }
        }
        None
    }
    
    /// Record a successful request (reset backoff)
    async fn record_success(&self, domain: &str) {
        self.consecutive_errors.write().await.remove(domain);
        self.backoff_until.write().await.remove(domain);
    }
    
    /// Record a rate-limited or error response (increase backoff)
    async fn record_rate_limit(&self, domain: &str) {
        let mut errors = self.consecutive_errors.write().await;
        let count = errors.entry(domain.to_string()).or_insert(0);
        *count += 1;
        
        // Exponential backoff: base_delay * 2^(error_count - 1)
        let backoff_ms = self.base_delay_ms * (1u64 << (*count - 1).min(10));
        let backoff_secs = (backoff_ms / 1000).min(self.max_backoff_secs);
        
        let until = Utc::now().timestamp() + backoff_secs as i64;
        self.backoff_until.write().await.insert(domain.to_string(), until);
    }
    
    /// Get current backoff status for a domain
    async fn get_backoff_info(&self, domain: &str) -> (u32, u64) {
        let errors = self.consecutive_errors.read().await;
        let count = *errors.get(domain).unwrap_or(&0);
        
        let backoff = self.backoff_until.read().await;
        let remaining = if let Some(&until) = backoff.get(domain) {
            let now = Utc::now().timestamp();
            if until > now { (until - now) as u64 } else { 0 }
        } else {
            0
        };
        
        (count, remaining)
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

// ═══════════════════════════════════════════════════════════════════════════════
// A-ADS IP FRESHNESS TRACKER (SQLite Backed)
// Tracks when each IP was last used to generate an A-Ads impression
// A-Ads only counts 1 impression per IP every 24 hours globally
// ═══════════════════════════════════════════════════════════════════════════════
use rusqlite::{Connection, params};
use std::sync::Mutex;

struct AadsIPTracker {
    /// SQLite connection (wrapped in Mutex for thread safety)
    db: Mutex<Connection>,
    /// Cooldown period in seconds (default: 24 hours = 86400)
    cooldown_seconds: i64,
    /// In-memory cache for frequently accessed IPs
    cache: RwLock<HashMap<String, i64>>,
}

impl AadsIPTracker {
    fn new(db_path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let conn = Connection::open(&db_path)?;
        
        // Create tables if not exist
        conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS ip_usage (
                ip TEXT PRIMARY KEY,
                last_used INTEGER NOT NULL,
                country TEXT,
                use_count INTEGER DEFAULT 1
            );
            
            CREATE TABLE IF NOT EXISTS geo_cache (
                ip TEXT PRIMARY KEY,
                country TEXT NOT NULL,
                cached_at INTEGER NOT NULL
            );
            
            CREATE INDEX IF NOT EXISTS idx_ip_last_used ON ip_usage(last_used);
            CREATE INDEX IF NOT EXISTS idx_geo_cached ON geo_cache(cached_at);
        "#)?;
        
        Ok(Self {
            db: Mutex::new(conn),
            cooldown_seconds: 86400, // 24 hours
            cache: RwLock::new(HashMap::new()),
        })
    }

    /// Load recent IPs into memory cache (call at startup)
    async fn load_cache(&self) -> Result<usize> {
        let now = Utc::now().timestamp();
        let cutoff = now - self.cooldown_seconds;
        
        let entries: Vec<(String, i64)> = {
            let db = self.db.lock().unwrap();
            let mut stmt = db.prepare("SELECT ip, last_used FROM ip_usage WHERE last_used > ?")?;
            let rows = stmt.query_map([cutoff], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };
        
        let count = entries.len();
        let mut cache = self.cache.write().await;
        for (ip, ts) in entries {
            cache.insert(ip, ts);
        }
        
        Ok(count)
    }

    /// Check if an IP can generate a unique impression
    async fn can_generate_unique(&self, ip: &str) -> bool {
        let now = Utc::now().timestamp();
        
        // Check memory cache first
        {
            let cache = self.cache.read().await;
            if let Some(&last_used) = cache.get(ip) {
                return (now - last_used) >= self.cooldown_seconds;
            }
        }
        
        // Check SQLite
        let last_used: Option<i64> = {
            let db = self.db.lock().unwrap();
            db.query_row(
                "SELECT last_used FROM ip_usage WHERE ip = ?",
                [ip],
                |row| row.get(0)
            ).ok()
        };
        
        match last_used {
            Some(ts) => (now - ts) >= self.cooldown_seconds,
            None => true, // Never used
        }
    }

    /// Mark an IP as used for A-Ads
    async fn mark_used(&self, ip: String, country: Option<String>) {
        let now = Utc::now().timestamp();
        
        // Update memory cache
        self.cache.write().await.insert(ip.clone(), now);
        
        // Update SQLite (upsert)
        let _ = {
            let db = self.db.lock().unwrap();
            db.execute(
                r#"INSERT INTO ip_usage (ip, last_used, country, use_count) 
                   VALUES (?1, ?2, ?3, 1)
                   ON CONFLICT(ip) DO UPDATE SET 
                     last_used = ?2, 
                     use_count = use_count + 1"#,
                params![ip, now, country]
            )
        };
    }

    /// Get freshness score for proxy selection
    async fn freshness_score(&self, ip: &str) -> u64 {
        let now = Utc::now().timestamp();
        
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(&last_used) = cache.get(ip) {
                let elapsed = now - last_used;
                return if elapsed >= self.cooldown_seconds { 500 } else { 0 };
            }
        }
        
        // Not in cache = never used = highest priority
        1000
    }

    /// Save cached country for an IP
    async fn save_geo(&self, ip: &str, country: &str) {
        let now = Utc::now().timestamp();
        let _ = {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT OR REPLACE INTO geo_cache (ip, country, cached_at) VALUES (?1, ?2, ?3)",
                params![ip, country, now]
            )
        };
    }

    /// Get cached country for an IP (7-day TTL)
    async fn get_geo(&self, ip: &str) -> Option<String> {
        const GEO_TTL: i64 = 7 * 24 * 3600; // 7 days
        let now = Utc::now().timestamp();
        let cutoff = now - GEO_TTL;
        
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT country FROM geo_cache WHERE ip = ? AND cached_at > ?",
            params![ip, cutoff],
            |row| row.get(0)
        ).ok()
    }

    /// Cleanup old entries (>48h for ip_usage, >7d for geo_cache)
    async fn cleanup(&self) {
        let now = Utc::now().timestamp();
        let ip_cutoff = now - (self.cooldown_seconds * 2);
        let geo_cutoff = now - (7 * 24 * 3600);
        
        let _ = {
            let db = self.db.lock().unwrap();
            db.execute("DELETE FROM ip_usage WHERE last_used < ?", [ip_cutoff])
        };
        let _ = {
            let db = self.db.lock().unwrap();
            db.execute("DELETE FROM geo_cache WHERE cached_at < ?", [geo_cutoff])
        };
        
        // Clear memory cache of old entries
        self.cache.write().await.retain(|_, &mut ts| now - ts < self.cooldown_seconds * 2);
    }

    /// Get statistics: (total_tracked, fresh_available, burned)
    async fn stats(&self) -> (usize, usize) {
        let now = Utc::now().timestamp();
        let cutoff = now - self.cooldown_seconds;
        
        let (total, burned): (usize, usize) = {
            let db = self.db.lock().unwrap();
            let total: i64 = db.query_row("SELECT COUNT(*) FROM ip_usage", [], |row| row.get(0)).unwrap_or(0);
            let burned: i64 = db.query_row(
                "SELECT COUNT(*) FROM ip_usage WHERE last_used > ?", 
                [cutoff], 
                |row| row.get(0)
            ).unwrap_or(0);
            (total as usize, burned as usize)
        };
        
        let fresh = total.saturating_sub(burned);
        (total, fresh)
    }
    
    /// Get burned IPs count
    async fn burned_count(&self) -> usize {
        let now = Utc::now().timestamp();
        let cutoff = now - self.cooldown_seconds;
        
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT COUNT(*) FROM ip_usage WHERE last_used > ?",
            [cutoff],
            |row| row.get::<_, i64>(0)
        ).unwrap_or(0) as usize
    }
    
    /// Get country distribution stats
    async fn country_stats(&self) -> Vec<(String, i64)> {
        let now = Utc::now().timestamp();
        let cutoff = now - self.cooldown_seconds;
        
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT country, COUNT(*) as cnt FROM ip_usage 
             WHERE last_used > ? AND country IS NOT NULL 
             GROUP BY country ORDER BY cnt DESC LIMIT 10"
        ).ok();
        
        if let Some(ref mut stmt) = stmt {
            stmt.query_map([cutoff], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            }).ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
        } else {
            Vec::new()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TOR CIRCUIT MANAGER
// Handles aggressive circuit rotation via Tor ControlPort
// ═══════════════════════════════════════════════════════════════════════════════
struct TorCircuitManager {
    control_ports: Vec<u16>,
    last_rotation: RwLock<HashMap<u16, i64>>,
    rotation_interval_secs: i64,
}

impl TorCircuitManager {
    fn new(control_ports: Vec<u16>) -> Self {
        Self {
            control_ports,
            last_rotation: RwLock::new(HashMap::new()),
            rotation_interval_secs: 90, // Rotate every 90 seconds
        }
    }
    
    /// Force new circuit on specific Tor instance
    async fn rotate_circuit(&self, control_port: u16) -> Result<Option<String>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;
        
        // Connect to Tor ControlPort
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", control_port)).await?;
        
        // Authenticate (assumes CookieAuthentication or no auth)
        stream.write_all(b"AUTHENTICATE\r\n").await?;
        let mut buf = [0u8; 256];
        let n = stream.read(&mut buf).await?;
        let auth_response = String::from_utf8_lossy(&buf[..n]);
        
        if !auth_response.contains("250 OK") {
            return Err(anyhow::anyhow!("Tor auth failed: {}", auth_response.trim()));
        }
        
        // Send NEWNYM signal to get new circuit
        stream.write_all(b"SIGNAL NEWNYM\r\n").await?;
        let n = stream.read(&mut buf).await?;
        let newnym_response = String::from_utf8_lossy(&buf[..n]);
        
        if !newnym_response.contains("250 OK") {
            return Err(anyhow::anyhow!("NEWNYM failed: {}", newnym_response.trim()));
        }
        
        // Wait for circuit establishment
        sleep(Duration::from_secs(3)).await;
        
        // Update last rotation time
        self.last_rotation.write().await.insert(control_port, Utc::now().timestamp());
        
        // Verify new IP (optional, for logging)
        let new_ip = self.get_current_ip(control_port - 100).await.ok(); // SOCKS port = control - 100 typically
        
        Ok(new_ip)
    }
    
    /// Get current exit IP for a Tor instance
    async fn get_current_ip(&self, socks_port: u16) -> Result<String> {
        let proxy = reqwest::Proxy::all(format!("socks5://127.0.0.1:{}", socks_port))?;
        let client = reqwest::Client::builder()
            .proxy(proxy)
            .timeout(Duration::from_secs(10))
            .build()?;
            
        let ip = client.get("https://api.ipify.org")
            .send()
            .await?
            .text()
            .await?;
            
        Ok(ip.trim().to_string())
    }
    
    /// Check if rotation is needed for a port
    async fn needs_rotation(&self, control_port: u16) -> bool {
        let now = Utc::now().timestamp();
        let rotation_map = self.last_rotation.read().await;
        
        match rotation_map.get(&control_port) {
            Some(&last) => (now - last) >= self.rotation_interval_secs,
            None => true, // Never rotated
        }
    }
    
    /// Rotate all circuits that need it
    async fn rotate_all_stale(&self) {
        for &port in &self.control_ports {
            if self.needs_rotation(port).await {
                if let Ok(Some(new_ip)) = self.rotate_circuit(port).await {
                    println!("{} Tor:{} -> {}", "[ROTATE]".blue(), port, new_ip.green());
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GEOLOCATION FOR HIGH-CPM COUNTRIES
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if country has high CPM (tier 1)
fn is_high_cpm_country(country: &str) -> bool {
    matches!(country, "US" | "GB" | "CA" | "AU" | "DE" | "FR" | "NL" | "CH" | "NO" | "SE" | "DK")
}

/// Country CPM multiplier for scoring
fn country_cpm_score(country: &str) -> u64 {
    match country {
        "US" => 200,
        "GB" | "CA" | "AU" => 150,
        "DE" | "FR" | "NL" => 130,
        "CH" | "NO" | "SE" | "DK" => 140,
        "JP" | "SG" | "NZ" => 120,
        _ => 100,
    }
}

/// Geolocate a proxy IP using ip-api.com (cached)
async fn geolocate_proxy(
    ip: &str, 
    cache: &RwLock<HashMap<String, (String, i64)>>,
) -> Option<String> {
    const CACHE_TTL: i64 = 7 * 24 * 3600; // 7 days
    
    // Check cache first
    {
        let cache_read = cache.read().await;
        if let Some((country, ts)) = cache_read.get(ip) {
            let now = Utc::now().timestamp();
            if now - ts < CACHE_TTL {
                return Some(country.clone());
            }
        }
    }
    
    // Query ip-api.com (free, no key required, 45 req/min limit)
    let url = format!("http://ip-api.com/json/{}?fields=countryCode", ip);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
    
    let resp = client.get(&url).send().await.ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    let country = json.get("countryCode")?.as_str()?.to_string();
    
    // Cache result
    {
        let mut cache_write = cache.write().await;
        let now = Utc::now().timestamp();
        cache_write.insert(ip.to_string(), (country.clone(), now));
    }
    
    Some(country)
}

// ═══════════════════════════════════════════════════════════════════════════════
// SMART PROXY POOL (Phase 3)
struct SmartProxyPool {
    proxies: RwLock<Vec<RatedProxy>>,
}

impl SmartProxyPool {
    fn new(proxies: Vec<RatedProxy>) -> Self {
        Self {
            proxies: RwLock::new(proxies),
        }
    }
    
    async fn get_best_for(&self, domain: &str) -> Option<RatedProxy> {
        let mut proxies = self.proxies.write().await;
        if proxies.is_empty() {
            return None;
        }
        
        // Advanced selection: tournament selection
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::from_entropy();
        let mut best_idx = 0;
        let mut best_score = 0;
        
        let sample_size = 15.min(proxies.len());
        for _ in 0..sample_size {
            let idx = rng.gen_range(0..proxies.len());
            let score = proxies[idx].domain_score(domain);
            if score >= best_score {
                best_score = score;
                best_idx = idx;
            }
        }
        
        Some(proxies.remove(best_idx))
    }
    
    async fn return_proxy(&self, mut proxy: RatedProxy, success: bool, domain: Option<&str>) {
        if let Some(dom) = domain {
            let stats = proxy.domain_stats.entry(dom.to_string()).or_insert((0, 0));
            if success {
                stats.0 += 1;
                proxy.success_count += 1;
            } else {
                stats.1 += 1;
                proxy.fail_count += 1;
            }
        }
        
        proxy.last_used = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
            
        self.proxies.write().await.push(proxy);
    }
    
    async fn add_bulk(&self, new_proxies: Vec<RatedProxy>) {
        let mut proxies = self.proxies.write().await;
        proxies.extend(new_proxies);
    }
    
    async fn len(&self) -> usize {
        self.proxies.read().await.len()
    }
    
    /// Get best proxy prioritizing A-Ads IP freshness AND high-CPM countries
    /// Combines domain performance with IP freshness for unique impressions
    async fn get_best_for_aads(&self, domain: &str, tracker: &AadsIPTracker) -> Option<RatedProxy> {
        let mut proxies = self.proxies.write().await;
        if proxies.is_empty() {
            return None;
        }
        
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::from_entropy();
        let mut best_idx = 0;
        let mut best_score = 0u64;
        
        // Larger sample for better A-Ads selection
        let sample_size = 25.min(proxies.len());
        
        for _ in 0..sample_size {
            let idx = rng.gen_range(0..proxies.len());
            let proxy = &proxies[idx];
            
            // Extract IP from proxy address (format: ip:port)
            let ip = proxy.addr.split(':').next().unwrap_or(&proxy.addr);
            
            // Get A-Ads freshness score (0, 500, or 1000)
            let freshness = tracker.freshness_score(ip).await;
            
            // Skip proxies with IPs in cooldown entirely (freshness = 0)
            if freshness == 0 {
                continue;
            }
            
            // Get country CPM score (0 for unknown, 100-500 for known)
            let cpm_score = match &proxy.country {
                Some(c) => country_cpm_score(c),
                None => {
                    // Try to get from tracker cache
                    if let Some(cached_country) = tracker.get_geo(ip).await {
                        country_cpm_score(&cached_country)
                    } else {
                        100 // Unknown country = baseline
                    }
                }
            };
            
            // Combine: domain score + freshness*3 + cpm_score
            // Freshness is most important, then CPM, then domain history
            let domain_score = proxy.domain_score(domain);
            let combined = domain_score + freshness * 3 + cpm_score;
            
            if combined >= best_score {
                best_score = combined;
                best_idx = idx;
            }
        }
        
        // If no proxy found with good freshness, fall back to any available
        if best_score == 0 && !proxies.is_empty() {
            best_idx = rng.gen_range(0..proxies.len());
        }
        
        Some(proxies.remove(best_idx))
    }
}

struct AppState {
    proxies: Arc<SmartProxyPool>,
    aads_tracker: Arc<AadsIPTracker>,
    tor_manager: Option<Arc<TorCircuitManager>>,
    rate_limiter: Arc<RateLimiter>,  // Phase 3: Exponential backoff
    burned_ips: RwLock<HashSet<String>>,
    daily_ips: RwLock<HashSet<String>>,
    blacklist: Arc<ProxyBlacklist>,
    stats: Arc<Stats>,
    target_url: String,
    tor_ports: Vec<u16>,
    tor_control_ports: Vec<u16>,
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
    aads_mode: bool,           // Enable A-Ads unique IP optimization
    cpm_estimate: f64,         // Estimated CPM for revenue calculation
    auto_stop_enabled: bool,   // Phase 3: Auto-stop when unique rate <10% for 30 mins
    auto_stop_threshold: f64,  // Unique rate threshold (default 10%)
    auto_stop_duration: u64,   // Duration in seconds (default 1800 = 30 mins)
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
    
    // PopAds - Enhanced detection for obfuscated scripts
    // Patterns: betteradsystem.com, cloudfront.net CDN, siteId+minBid+popundersPerIP
    if url_lower.contains("popads.net") 
        || body_lower.contains("popads.net")
        || body_lower.contains("betteradsystem.com")
        || body_lower.contains("data-cfasync=\"false\"")
        || (body_lower.contains("siteid") && body_lower.contains("minbid") && body_lower.contains("popundersperip"))
        || (body_lower.contains("cloudfront.net") && body_lower.contains("popunders"))
        || body_lower.contains("topmostlayer")
        || (body_lower.contains("atob(") && body_lower.contains("delaybetween")) {
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
async fn load_gold_proxies() -> Vec<RatedProxy> {
    let mut proxies = Vec::new();
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
                    proxies.push(proxy);
                }
            }
        }
    }
    
    if !proxies.is_empty() {
        println!("{}", format!("[✓] Loaded {} gold proxies", proxies.len()).green());
    }
    proxies
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
async fn fetch_and_validate_proxies(
    stats: Arc<Stats>,
    quiet: bool,
) -> Vec<RatedProxy> {
    let urls = vec![
        "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=http&timeout=10000&country=all&ssl=all&anonymity=all",
        "https://www.proxy-list.download/api/v1/get?type=http",
        "https://www.proxyscan.io/download?type=http",
    ];
    
    let mut all_proxies = Vec::new();
    for url in urls {
        match reqwest::get(url).await {
            Ok(resp) => {
                if let Ok(text) = resp.text().await {
                    for line in text.lines() {
                        if !line.trim().is_empty() {
                            all_proxies.push((line.trim().to_string(), ProxyType::Http));
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }
    
    // Deduplicate
    let mut seen = HashSet::new();
    let unique: Vec<(String, ProxyType)> = all_proxies
        .into_iter()
        .filter(|(addr, _)| seen.insert(addr.clone()))
        .collect();
        
    let mut validated = Vec::new();
    for (addr, ptype) in unique {
        if let Some(proxy) = validate_proxy(&addr, ptype).await {
            stats.validated.fetch_add(1, Ordering::Relaxed);
            validated.push(proxy);
        }
    }
    validated
}

/// Fetch and validate proxies with configurable thread count
async fn fetch_and_validate_proxies_threaded(
    threads: usize,
    stats: Arc<Stats>,
    quiet: bool,
) -> Vec<RatedProxy> {
    // Phase 4: Use specialized sources if detected (Placeholder for future)
    
    let sources = vec![
        "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=http&timeout=10000&country=all&ssl=all&anonymity=all",
        "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=socks4&timeout=10000&country=all",
        "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=socks5&timeout=10000&country=all",
        "https://www.proxy-list.download/api/v1/get?type=http",
        "https://www.proxy-list.download/api/v1/get?type=socks4",
        "https://www.proxy-list.download/api/v1/get?type=socks5",
        "https://spys.me/proxy.txt",
        "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/http.txt",
        "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks4.txt",
        "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks5.txt",
        "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/http.txt",
        "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/socks4.txt",
        "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/socks5.txt",
    ];
    
    let mut handles = Vec::new();
    for url in sources {
        handles.push(tokio::spawn(async move {
            match reqwest::get(url).await {
                Ok(resp) => {
                    if let Ok(text) = resp.text().await {
                        let ptype = if url.contains("socks5") {
                            ProxyType::Socks5
                        } else if url.contains("socks4") {
                            ProxyType::Socks4
                        } else {
                            ProxyType::Http
                        };
                        
                        text.lines()
                            .filter(|l| !l.trim().is_empty())
                            .map(|l| (l.trim().to_string(), ptype))
                            .collect::<Vec<(String, ProxyType)>>()
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
    
    let mut validated_list = Vec::new();
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
                
                validated_list.push(proxy);
                
                if validated_list.len() >= MAX_PROXIES_CACHE {
                    break;
                }
            }
        }
    }
    
    if !quiet {
        println!("{}", format!("[✓] {} validated", validated_list.len()).green());
    }
    validated_list
}

// ═══════════════════════════════════════════════════════════════════════════════
// REQUEST WITH RETRY
// ═══════════════════════════════════════════════════════════════════════════════
async fn request_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
    stats: &Arc<Stats>,
) -> anyhow::Result<reqwest::Response> {
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
    
    Err(last_error.unwrap().into())
}

// ═══════════════════════════════════════════════════════════════════════════════
// WORKER
// ═══════════════════════════════════════════════════════════════════════════════
async fn worker(id: usize, state: Arc<AppState>, semaphore: Arc<Semaphore>) {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::from_entropy();
    
    // Extract domain for rate limiting
    let domain = state.target_url.split('/')
        .nth(2)
        .unwrap_or("unknown")
        .to_string();
    
    loop {
        if state.shutdown.load(Ordering::Relaxed) {
            break;
        }
        
        let _permit = semaphore.acquire().await.unwrap();
        
        // Phase 3: Check RateLimiter before proceeding
        if let Some(wait_duration) = state.rate_limiter.should_wait(&domain).await {
            if id == 0 && !state.config.quiet {
                println!("{} Domain {} backed off for {}s", 
                    "[BACKOFF]".yellow(), domain, wait_duration.as_secs());
            }
            sleep(wait_duration.min(Duration::from_secs(30))).await;
        }
        
        // Phase 3: Human-like delay with log-normal variance (replaces simple random)
        sleep(human_delay(state.config.delay_ms)).await;
        
        // 60% Tor, 40% Proxy (when Tor available)
        let use_tor = !state.tor_ports.is_empty() && rng.gen_bool(0.60);
        
        let (client, proxy_obj_opt, proxy_addr) = if use_tor {
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
            (c, None, format!("tor:{}", port))
        } else {
            // Use A-Ads optimized selection if in aads_mode
            let proxy_info = if state.config.aads_mode {
                state.proxies.get_best_for_aads(&state.target_url, &state.aads_tracker).await
            } else {
                state.proxies.get_best_for(&state.target_url).await
            };
            
            match proxy_info {
                Some(proxy) => {
                    // Phase 3: Check ProxyBlacklist (temporarily failed proxies)
                    if state.blacklist.is_blacklisted(&proxy.addr).await {
                        state.proxies.return_proxy(proxy, false, None).await;
                        continue;
                    }
                    
                    if state.burned_ips.read().await.contains(&proxy.addr) {
                        state.proxies.return_proxy(proxy, false, None).await;
                        continue;
                    }
                    
                    let proxy_url = match proxy.proxy_type {
                        ProxyType::Http => format!("http://{}", &proxy.addr),
                        ProxyType::Socks5 => format!("socks5://{}", &proxy.addr),
                        ProxyType::Socks4 => format!("socks4://{}", &proxy.addr),
                    };
                    
                    let proxy_req_obj = match reqwest::Proxy::all(&proxy_url) {
                        Ok(p) => p,
                        Err(_) => {
                            state.proxies.return_proxy(proxy, false, None).await;
                            continue;
                        }
                    };
                    
                    let c = match reqwest::Client::builder()
                        .proxy(proxy_req_obj)
                        .timeout(Duration::from_secs(state.config.timeout_sec))
                        .pool_max_idle_per_host(5)
                        .build() {
                            Ok(c) => c,
                            Err(_) => {
                                state.proxies.return_proxy(proxy, false, None).await;
                                continue;
                            }
                    };
                    
                    let addr = proxy.addr.clone();
                    (c, Some(proxy), addr)
                }
                None => continue,
            }
        };
        
        // Generate realistic fingerprint
        let profile = &PROFILES[rng.gen_range(0..PROFILES.len())];
        let chrome_major: u32 = rng.gen_range(120..127);
        
        let referer = get_organic_referer();
        
        // Build Chrome-accurate headers
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", profile.ua_template.replacen("{}", &chrome_major.to_string(), 1).parse().unwrap());
        headers.insert("Referer", referer.parse().unwrap());
        
        // Add random Sec-Fetch headers
        let is_iframe = rng.gen_bool(0.2); // 20% chance to simulate iframe load
        let sec_headers = get_sec_fetch_headers_str(is_iframe);
        for (k, v) in sec_headers {
            if !v.is_empty() {
                headers.insert(k, v.parse().unwrap());
            }
        }
        
        // Add Client Hints
        let brands = profile.brands.replace("{}", &chrome_major.to_string());
        if profile.platform != "iOS" {
            headers.insert("Sec-CH-UA", brands.parse().unwrap());
            headers.insert("Sec-CH-UA-Mobile", if profile.mobile { "?1" } else { "?0" }.parse().unwrap());
            headers.insert("Sec-CH-UA-Platform", format!("\"{}\"", profile.platform).parse().unwrap());
        }

        // Add standard headers
        headers.insert("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8".parse().unwrap());
        headers.insert("Accept-Language", ACCEPT_LANGUAGES[rng.gen_range(0..ACCEPT_LANGUAGES.len())].parse().unwrap());
        headers.insert("Upgrade-Insecure-Requests", "1".parse().unwrap());
        
        let request_result = request_with_retry(&client, &state.target_url, headers.clone(), &state.stats).await;
        
        let success = match request_result {
            Ok(resp) => {
                let status = resp.status().as_u16();
                
                // Phase 3: Rate limiter tracking based on response code
                if status == 429 || (500..600).contains(&status) {
                    state.rate_limiter.record_rate_limit(&domain).await;
                    state.stats.rate_limited_count.fetch_add(1, Ordering::Relaxed);
                    if id == 0 && !state.config.quiet {
                        let (err_count, backoff) = state.rate_limiter.get_backoff_info(&domain).await;
                        println!("{} {} (errors: {}, backoff: {}s)", 
                            "[RATE-LIMITED]".red(), status, err_count, backoff);
                    }
                } else if (200..400).contains(&status) {
                    state.rate_limiter.record_success(&domain).await;
                }
                
                // Phase 5: Stream response instead of buffering (Memory Optimization)
                let mut body_str = String::new();
                let mut stream = resp.bytes_stream();
                use futures::StreamExt;
                
                // Limit body read to 128KB to prevent OOM on Termux
                let max_body_size = 128 * 1024;
                let mut bytes_read = 0;
                
                while let Some(chunk_result) = stream.next().await {
                    if let Ok(chunk) = chunk_result {
                        bytes_read += chunk.len();
                        body_str.push_str(&String::from_utf8_lossy(&chunk));
                        if bytes_read >= max_body_size {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                
                let is_success = (200..400).contains(&status);
                
                if is_success {
                    state.stats.impressions.fetch_add(1, Ordering::Relaxed);
                    
                    // A-ADS UNIQUE IMPRESSION TRACKING (CRITICAL FOR EARNINGS)
                    if state.config.aads_mode {
                        // Extract IP from proxy address
                        let ip = proxy_addr.split(':').next().unwrap_or(&proxy_addr);
                        
                        // Check if this IP can generate a unique impression
                        let is_unique = state.aads_tracker.can_generate_unique(ip).await;
                        
                        if is_unique {
                            // Mark as used BEFORE counting (prevents race conditions)
                            state.aads_tracker.mark_used(ip.to_string(), None).await;
                            state.stats.aads_unique_impressions.fetch_add(1, Ordering::Relaxed);
                            
                            if id == 0 && !state.config.quiet {
                                println!("{} {} via {}", "[UNIQUE]".green().bold(), status, proxy_addr.dimmed());
                            }
                        } else {
                            state.stats.aads_non_unique_impressions.fetch_add(1, Ordering::Relaxed);
                            
                            if id == 0 && !state.config.quiet {
                                println!("{} {} via {} (IP en cooldown)", "[WASTED]".yellow(), status, proxy_addr.dimmed());
                            }
                        }
                    }
                    
                    // Enhanced Ad Network Detection
                    if let Some(network) = detect_ad_network(&state.target_url, &body_str) {
                        match network {
                            "aads" => { state.stats.aads_hits.fetch_add(1, Ordering::Relaxed); },
                            "monetag" => { state.stats.monetag_hits.fetch_add(1, Ordering::Relaxed); },
                            _ => {} // Track others in generic stats if needed
                        };
                        
                        if !state.config.quiet && id == 0 && !state.config.aads_mode {
                            println!("{} {} [{}]", "[AD]".green(), network.cyan(), status);
                        }
                    }
                    
                    // Check for popunder triggers
                    // Check for popunder triggers (case-insensitive)
                    let body_check = body_str.to_lowercase();
                    if body_check.contains("onclick") || body_check.contains("popunder") 
                       || body_check.contains("window.open") || body_check.contains("popads")
                       || body_check.contains("cfasync") {
                        state.stats.popunder_triggers.fetch_add(1, Ordering::Relaxed);
                    }
                    
                    // Log success
                    if id == 0 && !state.config.quiet {
                        println!("{} {} via {}", "[HIT]".green(), status, proxy_addr.dimmed());
                    }
                    
                    // Human behavior: Dwell time
                    sleep(simulate_dwell_time()).await;
                    
                    // Human behavior: Click simulation
                    let base_rate = state.config.click_rate;
                    let network = detect_ad_network(&state.target_url, &body_str);
                    let effective_click_rate = get_network_click_rate(network, base_rate);
                    
                    if rng.gen::<f32>() < effective_click_rate {
                        // Organic click delay (human reaction time)
                        sleep(organic_click_delay()).await;
                        
                        let _ = request_with_retry(&client, &state.target_url, headers, &state.stats).await;
                        state.stats.clicks.fetch_add(1, Ordering::Relaxed);

                        if id == 0 && !state.config.quiet {
                             println!("{} via {}", "[CLICK]".yellow(), proxy_addr.dimmed());
                        }
                    }
                } else {
                    state.stats.errors.fetch_add(1, Ordering::Relaxed);
                }
                is_success
            }
            Err(_) => {
                state.stats.errors.fetch_add(1, Ordering::Relaxed);
                // Phase 3: Blacklist proxy on connection failure (5 min TTL)
                if let Some(ref proxy) = proxy_obj_opt {
                    state.blacklist.add(&proxy.addr).await;
                }
                false
            }
        };

        // Phase 3: Return proxy to pool with stats
        if let Some(proxy) = proxy_obj_opt {
            state.proxies.return_proxy(proxy, success, Some(&domain)).await;
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
        
        // A-Ads specific metrics
        let unique = state.stats.aads_unique_impressions.load(Ordering::Relaxed);
        let non_unique = state.stats.aads_non_unique_impressions.load(Ordering::Relaxed);
        let total_aads = unique + non_unique;
        let unique_rate = if total_aads > 0 { 
            (unique as f64 / total_aads as f64) * 100.0 
        } else { 
            100.0 // Assume 100% if no data yet
        };
        let estimated_revenue = unique as f64 * state.config.cpm_estimate;
        
        // IP Tracker stats
        let (tracked_ips, fresh_ips) = state.aads_tracker.stats().await;
        let burned_ips = state.aads_tracker.burned_count().await;
        
        // General metrics
        let rpm = if secs > 0.0 { (i as f64 / secs) * 60.0 } else { 0.0 };
        let ctr = if i > 0 { (c as f64 / i as f64) * 100.0 } else { 0.0 };
        let err_rate = if i + e > 0 { (e as f64 / (i + e) as f64) * 100.0 } else { 0.0 };
        
        let proxy_count = state.proxies.len().await;
        let tor_status = if state.tor_ports.is_empty() { "OFF" } else { "ON" };
        
        // Clear screen (works on Termux)
        print!("\x1b[H\x1b[2J");
        
        let title = if state.config.aads_mode {
            if is_term { "ICE COLD v38 [A-ADS]" } else { "ICE COLD v38 [A-ADS MODE]" }
        } else {
            if is_term { "ICE COLD v38 [TERMUX]" } else { "ICE COLD v38 [RUST]" }
        };
        
        // Main dashboard header
        println!("{}", format!("╔═══════════════ {} ═══════════════╗", title).magenta().bold());
        println!("{}", format!("║ Workers: {:>4} │ Proxies: {:>5} │ Tor: {:>3}   ║", 
            state.config.workers, proxy_count, tor_status).magenta());
        
        if state.config.aads_mode {
            // A-ADS SPECIFIC DASHBOARD
            println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
            println!("{}", "║         >>> A-ADS UNIQUE IMPRESSIONS <<<          ║".green().bold());
            println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
            
            // Unique impressions (the ONLY metric that matters for A-Ads)
            println!("{}", format!("║  ★ UNIQUE IMPRESSIONS:         {:>10}       ║", unique).green().bold());
            println!("{}", format!("║    Non-Unique (wasted):        {:>10}       ║", non_unique).yellow());
            
            // Unique rate with color coding
            let rate_str = format!("{:>5.1}%", unique_rate);
            let rate_display = if unique_rate >= 50.0 { 
                rate_str.green().bold()
            } else if unique_rate >= 30.0 { 
                rate_str.yellow()
            } else { 
                rate_str.red().bold()
            };
            println!("║    Unique Rate:                {}             ║", rate_display);
            
            // Estimated revenue
            println!("{}", format!("║    Est. Revenue:               ${:>8.4}        ║", estimated_revenue).cyan());
            
            println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
            println!("{}", "║              >>> IP TRACKING <<<                  ║".blue());
            println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
            println!("{}", format!("║  IPs Tracked (24h):            {:>10}       ║", tracked_ips).blue());
            println!("{}", format!("║  Fresh IPs Available:          {:>10}       ║", fresh_ips).green());
            println!("{}", format!("║  Burned IPs (in cooldown):     {:>10}       ║", burned_ips).red());
        }
        
        // General stats section
        println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
        println!("{}", format!("║  IMP: {:>8} │ CLK: {:>6} │ CTR: {:>5.2}%    ║", i, c, ctr).cyan());
        println!("{}", format!("║  ERR: {:>8} │ RPM: {:>6.0} │ ERR%: {:>4.1}%     ║", e, rpm, err_rate).cyan());
        println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
        println!("{}", format!("║  VAL: {:>8} │ GOLD: {:>5} │ RETRY: {:>5}  ║", v, g, r).yellow());
        println!("{}", format!("║  Runtime: {:02}h {:02}m {:02}s                          ║", 
            hrs, mins % 60, elapsed.as_secs() % 60).yellow());
        println!("{}", "╚═══════════════════════════════════════════════════╝".magenta());
        
        // A-Ads ALERTS & AUTO-STOP (Phase 3)
        if state.config.aads_mode && total_aads > 100 {
            if unique_rate < 10.0 {
                // Check if this is a sustained low rate
                let now = Utc::now().timestamp();
                let since = state.stats.low_unique_rate_since.load(Ordering::Relaxed);
                
                if since == 0 {
                    // First time detection
                    state.stats.low_unique_rate_since.store(now, Ordering::Relaxed);
                } else if state.config.auto_stop_enabled && (now - since) > state.config.auto_stop_duration as i64 {
                    // Auto-Stop triggered!
                    println!();
                    println!("{}", "╔═══════════════════════════════════════════════════╗".red().bold());
                    println!("{}", "║  🛑 AUTO-STOP TRIGGERED: LOW UNIQUE RATE          ║".red().bold());
                    println!("{}", "║  Unique rate <10% for >30 minutes.                ║".red());
                    println!("{}", "║  Stopping to preserve IPs and avoid bans.         ║".red());
                    println!("{}", "╚═══════════════════════════════════════════════════╝".red().bold());
                    
                    state.shutdown.store(true, Ordering::Relaxed);
                    break;
                }
                
                println!();
                println!("{}", "╔═══════════════════════════════════════════════════╗".red().bold());
                println!("{}", "║  ⚠️  ALERT: AFFILIATE MODE DETECTED!              ║".red().bold());
                println!("{}", "║  Unique rate <10% - A-Ads showing affiliate ads   ║".red());
                if state.config.auto_stop_enabled && since > 0 {
                    let remaining = state.config.auto_stop_duration as i64 - (now - since);
                    println!("║  Auto-stop in: {:>4}s                              ║", remaining.max(0).to_string().yellow());
                }
                println!("{}", "╚═══════════════════════════════════════════════════╝".red().bold());
            } else {
                // Reset timer if rate recovers
                state.stats.low_unique_rate_since.store(0, Ordering::Relaxed);
                
                if unique_rate < 30.0 {
                    println!();
                    println!("{}", "⚠️  WARNING: Unique rate <30% - Increase IP rotation!".yellow().bold());
                }
            }
        }
        
        // Country Stats (Phase 3)
        if state.config.aads_mode && elapsed.as_secs() % 10 == 0 {
             let countries = state.aads_tracker.country_stats().await;
             if !countries.is_empty() {
                 println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
                 println!("{}", "║              >>> TOP COUNTRIES <<<                ║".blue());
                 println!("{}", "╠═══════════════════════════════════════════════════╣".magenta());
                 for (i, (country, count)) in countries.iter().take(5).enumerate() {
                     println!("║  {}. {:<4} : {:>6} usage                          ║", i+1, country, count);
                 }
             }
        }
        
        // IP Exhaustion warning
        if state.config.aads_mode && burned_ips > proxy_count {
            println!();
            println!("{}", "⚠️  WARNING: More burned IPs than available proxies!".yellow());
            println!("{}", "    Consider: Adding more proxies or enabling Tor rotation".yellow());
        }
        
        // Save stats and cleanup tracker periodically (SQLite persists automatically)
        if elapsed.as_secs() % 60 == 0 {
            let _ = save_stats(&state.stats).await;
            state.aads_tracker.cleanup().await;
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
    
    // Phase 5: Config Presets for Termux
    println!("{}", "\nSelecciona un perfil de rendimiento:".cyan());
    println!("1. Tablet/Celu GAMA BAJA (20 workers, 1 Tor)");
    println!("2. Tablet/Celu GAMA ALTA (75 workers, 4 Tor)");
    println!("3. Personalizado [default]");
    
    let preset_choice = {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();
        if let Ok(Some(line)) = lines.next_line().await {
            line.trim().to_string()
        } else {
            "3".to_string()
        }
    };

    let (preset_workers, preset_tor) = match preset_choice.as_str() {
        "1" => (Some(20), Some(1)),
        "2" => (Some(75), Some(4)),
        _ => (None, None),
    };

    // Interactive: Ask for workers count if not specified via CLI/Preset
    let workers = if args.workers == 50 && config.workers.is_none() && preset_workers.is_none() {
        // Default value, ask user
        println!("{}", "\n¿Cuántos workers/hilos quieres usar? [default: 50]:".cyan());
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();
        
        if let Ok(Some(line)) = lines.next_line().await {
            line.trim().parse::<usize>().unwrap_or(50)
        } else {
            50
        }
    } else {
        preset_workers.or(config.workers).unwrap_or(args.workers)
    };
    println!("{}", format!("[✓] Workers: {}", workers).green());
    
    // Interactive: Ask for validation threads
    println!("{}", "\n¿Cuántos hilos para validar proxies? [default: 50]:".cyan());
    let validation_threads = {
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
    let tor_instances = preset_tor.or(config.tor_instances).unwrap_or(args.tor_instances);
    let tor_ports = if args.no_tor || config.no_tor.unwrap_or(false) {
        println!("{}", "[*] Tor disabled by config".yellow());
        vec![]
    } else {
        start_tor_pool(tor_instances).await?
    };
    
    // Initialize stats
    let stats = Arc::new(Stats::new());
    
    // Initialize A-Ads IP tracker (SQLite backed)
    let aads_tracker_path = get_data_dir().join("aads_tracker.db");
    let aads_tracker = Arc::new(AadsIPTracker::new(aads_tracker_path)?);
    
    // Load existing tracked IPs into cache
    match aads_tracker.load_cache().await {
        Ok(count) if count > 0 => {
            println!("{}", format!("[✓] Loaded {} tracked IPs from SQLite database", count).green());
        }
        _ => {}
    }
    
    // Detect if targeting A-Ads
    let aads_mode = target_url.contains("a-ads.com") || target_url.contains("ad.a-ads");
    if aads_mode {
        println!("{}", "[✓] A-ADS MODE ENABLED - Optimizing for unique impressions".green().bold());
    }
    
    // Load existing gold proxies and fetch new ones
    let mut gold_proxies = load_gold_proxies().await;
    let new_proxies = fetch_and_validate_proxies_threaded(validation_threads, stats.clone(), args.quiet).await;
    
    for proxy in new_proxies {
        gold_proxies.push(proxy);
    }
    
    // Generate control ports from socks ports
    let tor_control_ports: Vec<u16> = tor_ports.iter().enumerate()
        .map(|(i, _)| 9250 + i as u16)
        .collect();
    
    // Runtime config
    let runtime_config = RuntimeConfig {
        workers,
        click_rate: config.click_rate.unwrap_or(args.click_rate),
        delay_ms: config.delay_ms.unwrap_or(args.delay),
        timeout_sec: config.timeout_sec.unwrap_or(args.timeout),
        refresh_mins: config.refresh_mins.unwrap_or(args.refresh),
        quiet: args.quiet,
        aads_mode,
        cpm_estimate: 0.001, // $0.001 per unique impression (default)
        auto_stop_enabled: aads_mode, // Only enable auto-stop in A-Ads mode
        auto_stop_threshold: 10.0,    // Stop when unique rate < 10%
        auto_stop_duration: 1800,     // For 30 minutes
    };
    
    // Create proxy blacklist (5 min TTL)
    let blacklist = Arc::new(ProxyBlacklist::new(300));
    
    // Create Tor circuit manager if Tor is enabled
    let tor_manager = if !tor_control_ports.is_empty() {
        Some(Arc::new(TorCircuitManager::new(tor_control_ports.clone())))
    } else {
        None
    };
    
    // Create rate limiter for exponential backoff
    let rate_limiter = Arc::new(RateLimiter::new());
    
    // Create app state
    let state = Arc::new(AppState {
        proxies: Arc::new(SmartProxyPool::new(gold_proxies)),
        aads_tracker: aads_tracker.clone(),
        tor_manager: tor_manager.clone(),
        rate_limiter: rate_limiter.clone(),
        burned_ips: RwLock::new(HashSet::new()),
        daily_ips: RwLock::new(HashSet::new()),
        blacklist: blacklist.clone(),
        stats: stats.clone(),
        target_url,
        tor_ports,
        tor_control_ports,
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
            state_refresh.proxies.add_bulk(new).await;
        }
    });
    
    // Start monitor
    let state_monitor = state.clone();
    tokio::spawn(monitor(state_monitor, Instant::now()));
    
    // Start Tor circuit rotation task (if Tor enabled and A-Ads mode)
    if let Some(ref tor_mgr) = tor_manager {
        if aads_mode {
            let mgr = tor_mgr.clone();
            let shutdown_flag = state.clone();
            tokio::spawn(async move {
                loop {
                    sleep(Duration::from_secs(30)).await; // Check every 30s
                    if shutdown_flag.shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    mgr.rotate_all_stale().await;
                }
            });
            println!("{}", "[✓] Tor circuit rotation enabled (90s interval)".blue());
        }
    }
    
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
