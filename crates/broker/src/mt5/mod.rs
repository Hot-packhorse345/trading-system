mod bar_stream;
mod data_provider;
mod executor;
mod tick_stream;

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int, c_long};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};
use ts_core::{SymbolInfo, Timeframe};

pub(crate) const SYMBOL_INFO_TTL: Duration = Duration::from_secs(60);

// ── C-level structs ───────────────────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Mt5Rate {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: i64,
    pub spread: i32,
    pub _rv: i64, // real_volume — kept for ABI compat with MqlRates (60 bytes total)
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Mt5Tick {
    pub time: i64,
    pub bid: f64,
    pub ask: f64,
    pub last: f64,
    pub volume: u64,
    pub flags: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Mt5TradeResult {
    pub retcode: u32, // at offset 0 — aligned
    pub deal: u64,    // at offset 4 — misaligned without packed; DLL sends 36 bytes total
    pub order: u64,
    pub volume: f64,
    pub price: f64,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Mt5SymInfo {
    pub point: f64,
    pub tick_value: f64,
    pub lot_step: f64,
    pub min_lot: f64,
    pub max_lot: f64,
    pub spread: f64,
    pub digits: i32,
}

// ── DLL function type aliases ─────────────────────────────────────────────────

pub(crate) type FnInit = unsafe extern "C" fn(c_long, *const c_char, *const c_char) -> c_int;
pub(crate) type FnShut = unsafe extern "C" fn() -> c_int;
pub(crate) type FnRates =
    unsafe extern "C" fn(*const c_char, c_int, i64, i64, *mut Mt5Rate) -> c_int;
pub(crate) type FnAcct = unsafe extern "C" fn(*mut f64, *mut f64, *mut f64, *mut f64) -> c_int;
pub(crate) type FnSend = unsafe extern "C" fn(
    *const c_char,
    c_int,
    c_double,
    c_double,
    c_double,
    c_double,
    *const c_char,
    *mut Mt5TradeResult,
) -> c_int;
pub(crate) type FnClose = unsafe extern "C" fn(u64, *mut Mt5TradeResult) -> c_int;

pub(crate) type FnModify = unsafe extern "C" fn(u64, c_double, c_double) -> c_int;

pub(crate) type FnSymTick = unsafe extern "C" fn(*const c_char, *mut Mt5Tick) -> c_int;
pub(crate) type FnSymInfo = unsafe extern "C" fn(*const c_char, *mut Mt5SymInfo) -> c_int;

// ── Mt5Client ─────────────────────────────────────────────────────────────────

pub struct Mt5Client {
    _lib: libloading::Library,
    pub(crate) fn_init: FnInit,
    pub(crate) fn_shut: FnShut,
    pub(crate) fn_rates: FnRates,
    pub(crate) fn_acct: FnAcct,
    pub(crate) fn_send: FnSend,
    pub(crate) fn_close: FnClose,

    pub(crate) fn_modify: Option<FnModify>,

    pub(crate) fn_sym_tick: Option<FnSymTick>,
    pub(crate) fn_sym_info: Option<FnSymInfo>,

    pub(crate) symbol_cache: Arc<Mutex<HashMap<String, (SymbolInfo, Instant)>>>,
}

impl Mt5Client {
    pub fn connect(login: i64, password: &str, server: &str) -> Result<Self> {
        let dll = std::env::var("MT5_DLL_PATH").unwrap_or_else(|_| "MetaTrader5.dll".to_string());

        let lib = unsafe { libloading::Library::new(&dll) }
            .map_err(|e| anyhow!("Failed to load MT5 DLL '{dll}': {e}"))?;

        let client = unsafe {
            let fn_init: libloading::Symbol<FnInit> = lib.get(b"Initialize\0")?;
            let fn_shut: libloading::Symbol<FnShut> = lib.get(b"Shutdown\0")?;
            let fn_rates: libloading::Symbol<FnRates> = lib.get(b"CopyRates\0")?;
            let fn_acct: libloading::Symbol<FnAcct> = lib.get(b"AccountInfo\0")?;
            let fn_send: libloading::Symbol<FnSend> = lib.get(b"OrderSend\0")?;
            let fn_close: libloading::Symbol<FnClose> = lib.get(b"OrderClose\0")?;

            let fn_modify: Option<FnModify> = lib
                .get::<FnModify>(b"OrderModify\0")
                .map(|s| *s)
                .map_err(|_| warn!("MT5 DLL: 'OrderModify' not found — update_sl will error"))
                .ok();

            let fn_sym_tick: Option<FnSymTick> = lib
                .get::<FnSymTick>(b"SymbolInfoTick\0")
                .map(|s| *s)
                .map_err(|_| warn!("MT5 DLL: 'SymbolInfoTick' not found — TickStream unavailable"))
                .ok();

            let fn_sym_info: Option<FnSymInfo> = lib
                .get::<FnSymInfo>(b"SymbolInfoFull\0")
                .map(|s| *s)
                .map_err(|_| {
                    warn!("MT5 DLL: 'SymbolInfoFull' not found — symbol_info will use defaults")
                })
                .ok();

            Mt5Client {
                fn_init: *fn_init,
                fn_shut: *fn_shut,
                fn_rates: *fn_rates,
                fn_acct: *fn_acct,
                fn_send: *fn_send,
                fn_close: *fn_close,
                fn_modify,
                fn_sym_tick,
                fn_sym_info,
                symbol_cache: Arc::new(Mutex::new(HashMap::new())),
                _lib: lib,
            }
        };

        let pwd_c = CString::new(password)?;
        let srv_c = CString::new(server)?;
        let ret = unsafe { (client.fn_init)(login as c_long, pwd_c.as_ptr(), srv_c.as_ptr()) };
        if ret != 1 {
            return Err(anyhow!(
                "MT5 Initialize returned {ret} — check login/password/server"
            ));
        }
        info!("MT5 connected: login={login} server={server}");
        Ok(client)
    }

    pub(crate) fn tf_const(tf: Timeframe) -> c_int {
        match tf {
            Timeframe::M1 => 1,
            Timeframe::M2 => 2,
            Timeframe::M3 => 3,
            Timeframe::M5 => 5,
            Timeframe::M6 => 6,
            Timeframe::M10 => 10,
            Timeframe::M12 => 12,
            Timeframe::M15 => 15,
            Timeframe::M20 => 20,
            Timeframe::M30 => 30,
            Timeframe::H1 => 16385,
            Timeframe::H2 => 16386,
            Timeframe::H3 => 16387,
            Timeframe::H4 => 16388,
            Timeframe::H6 => 16390,
            Timeframe::H8 => 16392,
            Timeframe::H12 => 16396,
            Timeframe::D1 => 16408,
            Timeframe::W1 => 32769,
            Timeframe::MN1 => 49153,
            _ => 1,
        }
    }
}

impl Drop for Mt5Client {
    fn drop(&mut self) {
        unsafe {
            (self.fn_shut)();
        }
    }
}
