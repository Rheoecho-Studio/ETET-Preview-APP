//! Tauri commands: model picking, engine control, file reading, BYOK web
//! search and system info. Nothing here persists any data.
//! All heavy work (model load/unload, generation, file I/O) runs off the
//! main thread; the webview only forwards inputs and renders results.
use std::ffi::{c_char, CStr, CString};
use std::path::PathBuf;

use llama_sys::*;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::engine::{self, ChatMsg};

// ============================================================
// Path normalization.
//
// The webview hands us the path string returned by Tauri's file dialog.
// That string is NOT always a plain filesystem path:
//
//   - Android (Storage Access Framework) returns `content://` URIs. These
//     are not openable by llama.cpp's fopen/stat, so we resolve them via
//     JNI into the app cache dir (see resolve_android_content_uri).
//   - iOS returns a `file://` URL pointing into the app sandbox — usually
//     tmp/Inbox, which the OS is free to purge. We strip the scheme, decode
//     percent escapes, and (on mobile) copy the file into the persistent
//     cache so a later load cannot lose it to tmp cleanup.
//   - Desktop (Windows/macOS/Linux) dialogs return plain paths, passed
//     through unchanged. A stray `file://` is still handled uniformly.
//
// No runtime permission is required: Android relies on the picker's read
// grant, and on iOS the file is already inside the app sandbox.
// ============================================================

/// Normalize any path-ish input we get from the webview.
pub fn normalize_path(app: &tauri::AppHandle, input: &str) -> Result<String, String> {
    // Android SAF returns content:// URIs that are not openable as paths.
    #[cfg(target_os = "android")]
    {
        if input.starts_with("content://") {
            return resolve_android_content_uri(app, input)
                .map(|p| p.to_string_lossy().into_owned());
        }
    }

    // file:// URLs: strip the scheme + decode, then on mobile copy into the
    // persistent cache so a later load cannot lose the file to tmp cleanup.
    if let Some(rest) = input.strip_prefix("file://") {
        let fs_path = percent_decode(rest);
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let p = std::path::Path::new(&fs_path);
            if p.is_file() {
                if let Ok(cached) = copy_file_to_cache(app, p) {
                    return Ok(cached.to_string_lossy().into_owned());
                }
            }
        }
        return Ok(fs_path);
    }

    Ok(input.to_string())
}

/// Copy a real file path into the app cache dir (mobile only) and return it.
/// Models are multi-GB, so reuse the existing copy when the size matches.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn copy_file_to_cache(
    app: &tauri::AppHandle,
    src: &std::path::Path,
) -> Result<PathBuf, String> {
    use std::io::Write;

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("app cache dir: {e}"))?;
    let target_dir = cache_dir.join("etet_models");
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    let name = sanitize_name(
        &src
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model.gguf".to_string()),
    );
    let target = target_dir.join(&name);

    let src_len = src.metadata().map(|m| m.len()).unwrap_or(0);
    let cached_len = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    if src_len > 0 && src_len == cached_len {
        return Ok(target);
    }

    let mut src_file = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
    let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, out);
    std::io::copy(&mut src_file, &mut writer).map_err(|e| format!("copy model: {e}"))?;
    writer.flush().ok();
    Ok(target)
}

/// Copy a `content://` URI's bytes into the app cache dir and return the
/// real path. Runs entirely in Rust via JNI: the ContentResolver gives us a
/// ParcelFileDescriptor, whose fd we detach and copy with std::io.
///
/// No storage permission is needed: the document picker's read grant is
/// enough. The copy happens once per file (size-checked cache reuse).
#[cfg(target_os = "android")]
fn resolve_android_content_uri(app: &tauri::AppHandle, uri: &str) -> Result<PathBuf, String> {
    use jni::objects::{JObject, JValue};
    use jni::sys;
    use jni::JavaVM;
    use std::io::Write;
    use std::os::unix::io::FromRawFd;

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("app cache dir: {e}"))?;
    let target_dir = cache_dir.join("etet_models");
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    let ctx = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(ctx.vm() as *mut sys::JavaVM) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let context = unsafe { JObject::from_raw(ctx.context() as sys::jobject) };

    // NOTE: every JNI call below is a self-contained statement closed with
    // map_err. A shared `jni_check(&mut env, env.foo(..), ..)` helper cannot
    // be used here: it takes two mutable borrows of `env` in one expression,
    // which the borrow checker rejects (E0499 / E0502).
    let juri_str = env.new_string(uri).map_err(|e| e.to_string())?;
    let juri_obj: JObject = juri_str.into();
    let juri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&juri_obj)],
        )
        .map_err(|e| e.to_string())?
        .l()
        .map_err(|e| e.to_string())?;

    let resolver = env
        .call_method(
            &context,
            "getContentResolver",
            "()Landroid/content/ContentResolver;",
            &[],
        )
        .map_err(|e| e.to_string())?
        .l()
        .map_err(|e| e.to_string())?;

    let jmode: JObject = env.new_string("r").map_err(|e| e.to_string())?.into();
    let pfd = env
        .call_method(
            &resolver,
            "openFileDescriptor",
            "(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;",
            &[JValue::Object(&juri), JValue::Object(&jmode)],
        )
        .map_err(|e| e.to_string())?
        .l()
        .map_err(|e| e.to_string())?;

    // take ownership of the fd on the Rust side
    let raw_fd = env
        .call_method(&pfd, "detachFd", "()I", &[])
        .map_err(|e| e.to_string())?
        .i()
        .map_err(|e| e.to_string())?;
    if raw_fd < 0 {
        return Err(format!("detachFd returned {raw_fd}"));
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(raw_fd) };

    let name = sanitize_name(&content_uri_file_name(uri));
    let target = target_dir.join(&name);

    // reuse the cached copy when the size matches (models are multi-GB)
    let src_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let cached_len = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    if src_len > 0 && src_len == cached_len {
        return Ok(target);
    }

    let out =
        std::fs::File::create(&target).map_err(|e| format!("create {}: {e}", target.display()))?;
    let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, out);
    std::io::copy(&mut file, &mut writer).map_err(|e| format!("copy model: {e}"))?;
    writer.flush().ok();
    Ok(target)
}

/// Decode %XX escapes so a URI's tail becomes a usable file name.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// ".../document/primary%3ALLM%2FETET-x.gguf" -> "ETET-x.gguf"
fn content_uri_file_name(uri: &str) -> String {
    let tail = uri.rsplit('/').next().unwrap_or("");
    let decoded = percent_decode(tail);
    decoded
        .rsplit(|c| c == '/' || c == ':')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("model.gguf")
        .to_string()
}

fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Serialize)]
pub struct ModelInfo {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub arch: String,
    pub name: String,
    pub n_layers: u32,
    pub n_ctx_train: u32,
    pub mmproj_candidates: Vec<MmprojInfo>,
}

#[derive(Serialize)]
pub struct MmprojInfo {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
}

/// Parse GGUF metadata without loading the model into memory, and scan the
/// same directory for mmproj files.
#[tauri::command]
pub fn pick_model(app: tauri::AppHandle, path: String) -> Result<ModelInfo, String> {
    let path = normalize_path(&app, &path)?;
    let meta = PathBuf::from(&path);
    if !meta.is_file() {
        return Err(format!("not a file: {path}"));
    }

    let c_path = CString::new(path.clone()).map_err(|e| e.to_string())?;
    let ctx = unsafe {
        let mut params: gguf_init_params = std::mem::zeroed();
        params.no_alloc = true;
        gguf_init_from_file(c_path.as_ptr(), params)
    };
    if ctx.is_null() {
        return Err("not a valid GGUF file".into());
    }

    let read_str = |key: &str| -> String {
        unsafe {
            let ck = CString::new(key).unwrap();
            let key_id = gguf_find_key(ctx, ck.as_ptr());
            if key_id < 0 {
                return String::new();
            }
            let val = gguf_get_val_str(ctx, key_id);
            if val.is_null() {
                return String::new();
            }
            CStr::from_ptr(val).to_string_lossy().to_string()
        }
    };
    let read_u32 = |key: &str| -> u32 {
        unsafe {
            let ck = CString::new(key).unwrap();
            let key_id = gguf_find_key(ctx, ck.as_ptr());
            if key_id < 0 {
                return 0;
            }
            gguf_get_val_u32(ctx, key_id)
        }
    };

    let arch = read_str("general.architecture");
    let name = read_str("general.name");
    let n_layers = read_u32(&format!("{arch}.block_count"));
    let n_ctx_train = read_u32(&format!("{arch}.context_length"));
    unsafe { gguf_free(ctx) };

    // scan the model's directory for mmproj candidates
    let mut mmproj_candidates = Vec::new();
    if let Some(dir) = meta.parent() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let fname = entry.file_name().to_string_lossy().to_lowercase();
                if fname.ends_with(".gguf") && fname.contains("mmproj") {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    mmproj_candidates.push(MmprojInfo {
                        path: entry.path().to_string_lossy().to_string(),
                        file_name: entry.file_name().to_string_lossy().to_string(),
                        size_bytes: size,
                    });
                }
            }
        }
    }
    mmproj_candidates.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));

    Ok(ModelInfo {
        size_bytes: meta.metadata().map(|m| m.len()).unwrap_or(0),
        file_name: meta
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        path,
        arch,
        name,
        n_layers,
        n_ctx_train,
        mmproj_candidates,
    })
}

#[derive(Deserialize)]
pub struct LoadOptions {
    pub model_path: String,
    pub mmproj_path: Option<String>,
    pub n_ctx: u32,
    pub n_gpu_layers: u32,
    pub flash_attn: bool,
}

#[tauri::command]
pub async fn load_model(
    app: tauri::AppHandle,
    options: LoadOptions,
) -> Result<engine::LoadResult, String> {
    use tauri::Emitter;
    // Resolve content:// (Android) before the paths cross into the blocking
    // task, so the JNI calls happen on this (already attached) thread.
    let model_path = normalize_path(&app, &options.model_path)?;
    let mmproj_path = match &options.mmproj_path {
        Some(p) => Some(normalize_path(&app, p)?),
        None => None,
    };
    // long blocking load must stay off the main thread or the UI freezes
    let app2 = app.clone();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done2 = done.clone();
    // elapsed-seconds ticker so the UI can tell "loading" from "stuck"
    let ticker = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !done2.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if done2.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            let _ = app2.emit(
                "engine-progress",
                format!("t:{}", start.elapsed().as_secs()),
            );
        }
    });
    let result = tauri::async_runtime::spawn_blocking(move || {
        let r = engine::load(
            engine::LoadParams {
                model_path,
                mmproj_path,
                n_ctx: options.n_ctx,
                n_gpu_layers: options.n_gpu_layers,
                flash_attn: options.flash_attn,
            },
            &move |stage| {
                let _ = app.emit("engine-progress", stage);
            },
        );
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        r
    })
    .await
    .map_err(|e| format!("load task failed: {e}"))?;
    let _ = ticker.join();
    result
}

#[tauri::command]
pub async fn unload_model() {
    // freeing multi-GB weights also blocks; keep it off the main thread
    let _ = tauri::async_runtime::spawn_blocking(engine::unload).await;
}

#[derive(Deserialize)]
pub struct GenerateOptions {
    pub messages: Vec<ChatMsg>,
    pub images_b64: Vec<String>,
    pub max_tokens: i32,
    pub enable_thinking: bool,
}

/// Streams classified (reasoning/body) deltas as `token` events and finishes
/// with a `gen-done` event. Runs off the main thread.
#[tauri::command]
pub async fn generate(app: tauri::AppHandle, options: GenerateOptions) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        engine::generate(
            &options.messages,
            &options.images_b64,
            options.max_tokens,
            options.enable_thinking,
            &|reasoning, reset, text| {
                let _ = app.emit("token", TokenDelta { reasoning, reset, text });
            },
            &|stopped_by_user| {
                let _ = app.emit("gen-done", stopped_by_user);
            },
        )
    })
    .await
    .map_err(|e| format!("generation task failed: {e}"))?
}

#[derive(Serialize, Clone)]
pub struct TokenDelta {
    pub reasoning: bool,
    pub reset: bool,
    pub text: String,
}

#[tauri::command]
pub fn stop_generation() {
    engine::stop_generation();
}

/// Read a text file (Rust-side I/O + truncation to protect small contexts).
#[tauri::command]
pub fn read_file_text(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let path = normalize_path(&app, &path)?;
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut content = content;
    if content.chars().count() > 20_000 {
        content = content.chars().take(20_000).collect();
        content.push_str("\n…[truncated]");
    }
    Ok(content)
}

#[derive(Serialize)]
pub struct ImageData {
    pub file_name: String,
    pub b64: String,
}

/// Read an image file and return base64 (Rust-side I/O only).
#[tauri::command]
pub fn read_file_base64(app: tauri::AppHandle, path: String) -> Result<ImageData, String> {
    let path = normalize_path(&app, &path)?;
    let p = PathBuf::from(&path);
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    use base64::Engine;
    Ok(ImageData {
        file_name: p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        b64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[derive(Deserialize)]
pub struct WebSearchOptions {
    pub provider: String, // "tavily" | "brave" | "custom"
    pub api_key: String,
    pub url: Option<String>, // for custom provider
    pub query: String,
}

#[derive(Serialize, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// BYOK search proxy. Keys are only used in-memory for this single request.
#[tauri::command]
pub async fn web_search(options: WebSearchOptions) -> Result<Vec<SearchResult>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    match options.provider.as_str() {
        "tavily" => {
            let resp = client
                .post("https://api.tavily.com/search")
                .json(&serde_json::json!({
                    "api_key": options.api_key,
                    "query": options.query,
                    "max_results": 5,
                }))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            Ok(json["results"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|r| SearchResult {
                            title: r["title"].as_str().unwrap_or_default().to_string(),
                            url: r["url"].as_str().unwrap_or_default().to_string(),
                            snippet: r["content"].as_str().unwrap_or_default().to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default())
        }
        "brave" => {
            let resp = client
                .get("https://api.search.brave.com/res/v1/web/search")
                .header("X-Subscription-Token", &options.api_key)
                .header("Accept", "application/json")
                .query(&[("q", options.query.as_str()), ("count", "5")])
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            Ok(json["web"]["results"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|r| SearchResult {
                            title: r["title"].as_str().unwrap_or_default().to_string(),
                            url: r["url"].as_str().unwrap_or_default().to_string(),
                            snippet: r["description"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default())
        }
        "custom" => {
            let base = options.url.ok_or("custom provider needs a url")?;
            let full = if base.contains('?') {
                format!("{base}&q={}", urlencode(&options.query))
            } else {
                format!("{base}?q={}", urlencode(&options.query))
            };
            let mut req = client.get(&full);
            if !options.api_key.is_empty() {
                req = req.bearer_auth(&options.api_key);
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            // best-effort extraction from common response shapes
            let arr = json["results"]
                .as_array()
                .or_else(|| json["data"].as_array())
                .cloned()
                .unwrap_or_default();
            Ok(arr
                .iter()
                .map(|r| SearchResult {
                    title: r["title"].as_str().unwrap_or_default().to_string(),
                    url: (r["url"].as_str().or(r["link"].as_str()))
                        .unwrap_or_default()
                        .to_string(),
                    snippet: (r["snippet"]
                        .as_str()
                        .or(r["content"].as_str())
                        .or(r["description"].as_str()))
                    .unwrap_or_default()
                    .to_string(),
                })
                .collect())
        }
        other => Err(format!("unknown provider: {other}")),
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Serialize)]
pub struct SysInfo {
    pub total_mem: u64,
    pub available_mem: u64,
    pub gpu_backend: String,
}

#[tauri::command]
pub fn system_info() -> SysInfo {
    let gpu_backend = if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        "Metal".to_string()
    } else {
        "Vulkan".to_string()
    };

    #[cfg(not(target_os = "ios"))]
    {
        use sysinfo::System;
        let mut sys = System::new();
        sys.refresh_memory();
        SysInfo { total_mem: sys.total_memory(), available_mem: sys.available_memory(), gpu_backend }
    }
    // iOS 上 sysinfo 不可用，内存信息返回 0，前端别显示就行
    #[cfg(target_os = "ios")]
    {
        SysInfo { total_mem: 0, available_mem: 0, gpu_backend }
    }
}


#[allow(dead_code)]
fn _keep_imports(_: &c_char) {}