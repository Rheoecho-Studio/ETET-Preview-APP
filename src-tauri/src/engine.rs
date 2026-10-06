//! Engine: thin Rust glue over the C++ shim (llama-sys/shim), which drives
//! llama.cpp's official common/mtmd layers. All template, tokenization,
//! multimodal, sampling, stop and output-parsing logic lives in llama.cpp
//! itself — nothing is re-implemented here.
use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub struct Engine {
    inner: *mut c_void,
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

static ENGINE: OnceLock<Mutex<Option<Engine>>> = OnceLock::new();
static STOP: OnceLock<AtomicBool> = OnceLock::new();

fn engine_slot() -> &'static Mutex<Option<Engine>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

fn stop_flag() -> &'static AtomicBool {
    STOP.get_or_init(|| AtomicBool::new(false))
}

pub fn stop_generation() {
    stop_flag().store(true, Ordering::SeqCst);
}

pub struct LoadParams {
    pub model_path: String,
    pub mmproj_path: Option<String>,
    pub n_ctx: u32,
    pub n_gpu_layers: u32,
    pub flash_attn: bool,
}

#[derive(serde::Serialize)]
pub struct LoadResult {
    pub model_name: String,
    pub n_ctx: u32,
    pub n_params: u64,
    pub has_vision: bool,
    pub n_layers: i32,
}

// ---- C ABI of the shim (llama-sys/shim/shim.cpp) ----
type ShimProgressCb = unsafe extern "C" fn(stage: *const c_char, user: *mut c_void);
type ShimDeltaCb =
    unsafe extern "C" fn(user: *mut c_void, is_reasoning: c_int, reset: c_int, text: *const c_char);
type ShimStopPoll = unsafe extern "C" fn(user: *mut c_void) -> c_int;

extern "C" {
    fn shim_engine_init(
        model_path: *const c_char,
        mmproj_path: *const c_char,
        n_ctx: c_int,
        n_gpu_layers: c_int,
        flash_attn: c_int,
        progress_cb: ShimProgressCb,
        progress_user: *mut c_void,
    ) -> *mut c_void;
    fn shim_engine_free(e: *mut c_void);
    fn shim_has_vision(e: *mut c_void) -> bool;
    fn shim_generate(
        e: *mut c_void,
        roles: *const *const c_char,
        contents: *const *const c_char,
        n_msgs: usize,
        images: *const *const u8,
        image_lens: *const usize,
        n_images: usize,
        max_tokens: c_int,
        enable_thinking: c_int,
        delta_cb: ShimDeltaCb,
        delta_user: *mut c_void,
        stop_poll: ShimStopPoll,
        stop_user: *mut c_void,
    ) -> c_int;
}

// ---- callback trampolines ----
struct ProgressState<'a>(&'a dyn Fn(String));

unsafe extern "C" fn trampoline_progress(stage: *const c_char, user: *mut c_void) {
    let state = &*(user as *const ProgressState);
    let s = std::ffi::CStr::from_ptr(stage).to_string_lossy().to_string();
    state.0(s);
}

struct GenState<'a> {
    on_piece: &'a dyn Fn(bool, bool, String), // (reasoning, reset, text)
}

unsafe extern "C" fn trampoline_delta(
    user: *mut c_void,
    is_reasoning: c_int,
    reset: c_int,
    text: *const c_char,
) {
    let state = &*(user as *const GenState);
    let s = std::ffi::CStr::from_ptr(text).to_string_lossy().to_string();
    (state.on_piece)(is_reasoning != 0, reset != 0, s);
}

unsafe extern "C" fn trampoline_stop_poll(_user: *mut c_void) -> c_int {
    stop_flag().load(Ordering::SeqCst) as c_int
}

pub fn load(params: LoadParams, progress: &dyn Fn(String)) -> Result<LoadResult, String> {
    unload();
    let c_model = CString::new(params.model_path.clone()).map_err(|e| e.to_string())?;
    let c_mmproj = params
        .mmproj_path
        .as_ref()
        .map(|p| CString::new(p.clone()).map_err(|e| e.to_string()))
        .transpose()?;

    let mut state = ProgressState(progress);
    let inner = unsafe {
        shim_engine_init(
            c_model.as_ptr(),
            c_mmproj.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            params.n_ctx as c_int,
            params.n_gpu_layers as c_int,
            params.flash_attn as c_int,
            trampoline_progress,
            &mut state as *mut ProgressState as *mut c_void,
        )
    };
    if inner.is_null() {
        return Err("failed to load model".into());
    }
    let has_vision = unsafe { shim_has_vision(inner) };
    *engine_slot().lock().unwrap() = Some(Engine { inner });

    let model_name = std::path::Path::new(&params.model_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(LoadResult {
        model_name,
        n_ctx: params.n_ctx,
        n_params: 0,
        has_vision,
        n_layers: 0,
    })
}

pub fn unload() {
    let mut guard = engine_slot().lock().unwrap();
    if let Some(e) = guard.take() {
        unsafe { shim_engine_free(e.inner) };
    }
}

#[derive(serde::Deserialize, Clone)]
pub struct ChatMsg {
    pub role: String,
    pub content: String,
}

/// One generation turn. `on_piece(reasoning, reset, text)` per classified
/// delta; `on_done(stopped_by_user)` when the turn ends.
pub fn generate(
    messages: &[ChatMsg],
    images_b64: &[String],
    max_tokens: i32,
    enable_thinking: bool,
    on_piece: &dyn Fn(bool, bool, String),
    on_done: &dyn Fn(bool),
) -> Result<(), String> {
    use base64::Engine;
    let inner = with_engine(|e| e.map(|e| e.inner))
        .ok_or_else(|| "no model loaded".to_string())?;

    let mut keep: Vec<CString> = Vec::with_capacity(messages.len() * 2);
    let mut roles: Vec<*const c_char> = Vec::with_capacity(messages.len());
    let mut contents: Vec<*const c_char> = Vec::with_capacity(messages.len());
    for m in messages {
        let r = CString::new(m.role.clone()).map_err(|e| e.to_string())?;
        let c = CString::new(m.content.clone()).map_err(|e| e.to_string())?;
        roles.push(r.as_ptr());
        contents.push(c.as_ptr());
        keep.push(r);
        keep.push(c);
    }

    // images: base64 -> raw bytes; the shim decodes via mtmd_helper (stb)
    let mut image_bytes: Vec<Vec<u8>> = Vec::with_capacity(images_b64.len());
    let mut image_ptrs: Vec<*const u8> = Vec::with_capacity(images_b64.len());
    let mut image_lens: Vec<usize> = Vec::with_capacity(images_b64.len());
    for b64 in images_b64 {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| format!("base64 decode failed: {e}"))?;
        image_ptrs.push(bytes.as_ptr());
        image_lens.push(bytes.len());
        image_bytes.push(bytes);
    }

    stop_flag().store(false, Ordering::SeqCst);
    let state = GenState { on_piece };

    let rc = unsafe {
        shim_generate(
            inner,
            roles.as_ptr(),
            contents.as_ptr(),
            messages.len(),
            image_ptrs.as_ptr(),
            image_lens.as_ptr(),
            image_bytes.len(),
            max_tokens,
            enable_thinking as c_int,
            trampoline_delta,
            &state as *const GenState as *mut c_void,
            trampoline_stop_poll,
            &state as *const GenState as *mut c_void,
        )
    };
    let stopped = rc == 1;
    if rc < 0 {
        return Err(format!("generation failed (shim rc={rc})"));
    }
    on_done(stopped);
    Ok(())
}

fn with_engine<T>(f: impl FnOnce(Option<&Engine>) -> T) -> T {
    let guard = engine_slot().lock().unwrap();
    f(guard.as_ref())
}

// keep c_char import used
#[allow(dead_code)]
fn _keep(_: &c_char) {}
