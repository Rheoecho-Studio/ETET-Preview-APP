// Builds the local llama.cpp (read-only: never modifies its sources) plus a
// small C++ shim over its official common/mtmd layers, and generates Rust FFI
// bindings for llama.h / gguf.h / mtmd.h.
//
// Targets covered:
//   desktop : Windows (msvc + gnu), macOS, Linux
//   mobile  : Android (arm64-v8a / armeabi-v7a / x86 / x86_64), iOS (device + simulator)
//
// Backend defaults: Windows/Linux -> vulkan (GPU), macOS/iOS device -> metal,
// Android/iOS simulator -> cpu. Windows needs a Vulkan SDK at build time.
//
// Optional environment knobs (every one has a sane default):
//   LLAMA_GPU_BACKEND   cpu | metal | vulkan | opencl   (default: chosen per platform)
//   ANDROID_NDK_HOME / ANDROID_NDK_ROOT / ANDROID_NDK / NDK_HOME   (Android only)
//   ANDROID_PLATFORM    default "android-30" (Android 11 minSdk)     (Android only)
//   IOS_MIN_VERSION     default "14.0"       (iOS 14 min target)     (iOS only)
//
// Build against the newest SDK (Android 17 / iOS 27) while keeping the
// deployment floor at Android 11 / iOS 14, as required by the app.
use std::env;
use std::path::{Path, PathBuf};

/// First non-empty value among the candidate env vars.
fn first_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|n| env::var_os(n))
        .map(PathBuf::from)
        .find(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().to_string())
}

/// Rust target triple -> Android ABI (the name cmake/gradle expect).
fn android_abi(target: &str) -> &'static str {
    if target.starts_with("aarch64") {
        "arm64-v8a"
    } else if target.starts_with("armv7") {
        "armeabi-v7a"
    } else if target.starts_with("i686") {
        "x86"
    } else if target.starts_with("x86_64") {
        "x86_64"
    } else {
        panic!("unsupported Android target: {target}")
    }
}

/// Rust target triple -> clang target triple including the API level, e.g.
/// `aarch64-linux-android28`. NDK clang needs the level on the triple.
fn android_clang_triple(target: &str, api: &str) -> String {
    if target.starts_with("armv7") {
        format!("armv7a-linux-androideabi{api}")
    } else {
        format!("{target}{api}")
    }
}

/// HOST triple -> NDK prebuilt toolchain directory name.
fn ndk_host_tag(host: &str) -> String {
    if host.contains("apple-darwin") {
        if host.starts_with("aarch64") {
            "darwin-arm64".to_string()
        } else {
            "darwin-x86_64".to_string()
        }
    } else if host.contains("windows") {
        "windows-x86_64".to_string()
    } else if host.starts_with("aarch64") {
        "linux-aarch64".to_string()
    } else {
        "linux-x86_64".to_string()
    }
}

fn ios_arch(target: &str) -> &'static str {
    if target.starts_with("aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

/// Does `dir` hold a static archive for `name`? MSVC writes `name.lib`, every other
/// toolchain writes `libname.a`.
fn has_static_lib(dir: &Path, name: &str) -> bool {
    dir.join(format!("{name}.lib")).exists() || dir.join(format!("lib{name}.a")).exists()
}

/// Short CMake work root used on Windows (see the comment at the call site).
/// `LLAMA_CMAKE_DIR` overrides it; otherwise it sits under TEMP so it is always
/// writable and never collides with anything.
fn windows_cmake_dir() -> PathBuf {
    match first_env(&["LLAMA_CMAKE_DIR"]) {
        Some(dir) => strip_verbatim(PathBuf::from(dir)),
        None => {
            let base = first_env(&["TEMP", "TMP"])
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows\Temp"));
            strip_verbatim(base.join("llama-cb"))
        }
    }
}

/// On Windows `Path::canonicalize()` returns a verbatim path (`\\?\D:\...`). CMake cannot
/// inspect such a path: ExternalProject_Add's own file(GLOB)/IS_DIRECTORY checks see it as
/// empty, so ggml-vulkan's `vulkan-shaders-gen` target fails with
/// "No download info given ... is not an existing non-empty directory" even though the
/// directory is fully populated (verified: 188 files). Drop the verbatim prefix so CMake
/// receives a normal path. Linux/macOS canonicalize never adds the prefix, which is why
/// only Windows was affected.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

fn main() {
    let target = env::var("TARGET").unwrap();
    let host = env::var("HOST").unwrap();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // ---- target classification ----
    let is_android = target.contains("android");
    let is_ios = target.contains("apple-ios");
    let is_macos = target.contains("apple-darwin");
    let is_windows = target.contains("windows");
    let is_msvc = target.contains("msvc");
    let is_apple = is_macos || is_ios;
    let is_mobile = is_android || is_ios;
    let is_ios_sim = is_ios
        && (target.starts_with("x86_64-")
            || target.starts_with("i686-")
            || target.ends_with("-sim"));

    // llama.cpp lives at the project root (sibling of src-tauri).
    // CI can override with LLAMA_CPP_DIR (its own checkout of the fork).
    let llama_dir = match env::var_os("LLAMA_CPP_DIR") {
        Some(dir) => strip_verbatim(PathBuf::from(dir)),
        None => strip_verbatim(
            manifest_dir
                .join("..")
                .join("..")
                .join("llama.cpp")
                .canonicalize()
                .expect("llama.cpp not found next to src-tauri; set LLAMA_CPP_DIR"),
        ),
    };
    println!("cargo:rerun-if-env-changed=LLAMA_CPP_DIR");
    println!("cargo:rerun-if-env-changed=LLAMA_GPU_BACKEND");
    println!("cargo:rerun-if-env-changed=ANDROID_PLATFORM");
    println!("cargo:rerun-if-env-changed=IOS_MIN_VERSION");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");
    println!("cargo:rerun-if-env-changed=VULKAN_SDK");
    println!("cargo:rerun-if-env-changed=VULKAN_HEADERS_INCLUDE");
    println!("cargo:rerun-if-env-changed=VULKAN_HPP_INCLUDE");
    println!("cargo:rerun-if-env-changed=SPIRV_HEADERS_INCLUDE");
    println!("cargo:rerun-if-env-changed=SPIRV_HEADERS_DIR");
    println!("cargo:rerun-if-env-changed=LLAMA_CMAKE_DIR");
    println!(
        "cargo:rerun-if-changed={}",
        llama_dir.join("include/llama.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("shim/shim.cpp").display()
    );

    // ---- backend selection ----
    // Desktop (Windows/Linux) and Android run on the GPU via Vulkan.
    // CPU-only is kept only for the iOS simulator, which has no Metal.
    let backend = env::var("LLAMA_GPU_BACKEND").unwrap_or_else(|_| {
        if is_ios {
            if is_ios_sim {
                "cpu".to_string() // no Metal in the simulator
            } else {
                "metal".to_string()
            }
        } else if is_macos {
            "metal".to_string()
        } else if is_android {
            "vulkan".to_string()
        } else {
            "vulkan".to_string() // Windows + Linux desktop
        }
    });

    let mut cfg = cmake::Config::new(&llama_dir);
    cfg.define("BUILD_SHARED_LIBS", "OFF")
        .define("LLAMA_BUILD_TESTS", "OFF")
        .define("LLAMA_BUILD_TOOLS", "ON")
        .define("LLAMA_BUILD_EXAMPLES", "OFF")
        .define("LLAMA_BUILD_SERVER", "OFF")
        .define("LLAMA_BUILD_APP", "OFF")
        .define("LLAMA_BUILD_UI", "OFF")
        .define("LLAMA_BUILD_MTMD", "ON")
        .define("LLAMA_CURL", "OFF")
        .define("GGML_LLAMAFILE", "OFF")
        .define("GGML_OPENMP", "OFF")
        .define("CMAKE_COMPILE_WARNING_AS_ERROR", "OFF")
        .profile("Release");

    // Windows: keep CMake's build tree out of the (very long) cargo OUT_DIR.
    // ggml-vulkan builds its shader generator with CMake's ExternalProject, and that runs a
    // nested `cmake` configure under
    //   <build>/ggml/src/ggml-vulkan/vulkan-shaders-gen-prefix/src/vulkan-shaders-gen-build/
    //     CMakeFiles/CMakeScratch/TryCompile-XXXXXX/
    // With OUT_DIR = D:\a\<repo>\<repo>\src-tauri\target\release\build\llama-sys-<hash>\out
    // that probe path reaches ~258 characters, right at the Windows MAX_PATH limit, and
    // cl.exe fails its compiler test with
    //   "error C1083: Cannot open compiler generated file: ''"
    // -> "Check for working C compiler ... is broken" -> "No CMAKE_CXX_COMPILER could be found".
    // A short root puts every generated path far below the limit. cmake-rs derives both the
    // build dir (<root>/build) and the install prefix (<root>) from this value, so nothing
    // downstream has to change.
    if is_windows {
        cfg.out_dir(windows_cmake_dir());
    }

    // Cross-compiling: -march=native cannot work for a foreign CPU.
    if is_mobile {
        cfg.define("GGML_NATIVE", "OFF");
    }

    // KEY: we must not define only the one backend we selected. ggml-backend-reg.cpp
    // compiles its registry from these cmake switches; any backend not explicitly turned
    // OFF (llama.cpp enables some by platform default) gets a *_reg reference compiled in,
    // which then fails to link because the static lib is absent:
    //   macOS / iOS -> "_ggml_backend_blas_reg"
    //   iOS simulator -> "_ggml_backend_metal_reg"
    cfg.define("GGML_BLAS", "OFF");
    cfg.define("GGML_METAL", if backend == "metal" { "ON" } else { "OFF" });
    cfg.define("GGML_VULKAN", if backend == "vulkan" { "ON" } else { "OFF" });
    cfg.define("GGML_OPENCL", if backend == "opencl" { "ON" } else { "OFF" });
    // ggml-cpu uses vDSP (Accelerate) on Apple platforms by default. macOS already links
    // Accelerate.framework explicitly at the Rust link step, so leaving it ON is fine there;
    // but iOS links inside the Xcode project, which does not auto-add Accelerate.framework,
    // causing "ld: symbol(s) not found: _vDSP_vadd" etc. Turn it OFF for iOS so ggml-cpu
    // uses its pure-C path and needs no Accelerate at link time.
    cfg.define("GGML_ACCELERATE", if is_ios { "OFF" } else { "ON" });
    if backend == "metal" && is_ios && !is_ios_sim {
        // iOS apps cannot ship a loose .metallib; embed it in the binary.
        cfg.define("GGML_METAL_EMBED_LIBRARY", "ON");
    }

    // ---- per-platform cross-compile settings ----
    // Filled in for Android so the shim can be compiled with the same NDK
    // toolchain (and so the NDK sysroot can be added to the link search path).
    let mut android: Option<(PathBuf, String)> = None; // (sysroot, clang triple)

    if is_android {
        let ndk = first_env(&[
            "ANDROID_NDK_HOME",
            "ANDROID_NDK_ROOT",
            "ANDROID_NDK",
            "NDK_HOME",
        ])
        .map(PathBuf::from)
        .expect("Android build needs ANDROID_NDK_HOME (or ANDROID_NDK_ROOT) pointing at the NDK");
        // android-30 == Android 11, the app's minimum supported version.
        let platform = env::var("ANDROID_PLATFORM").unwrap_or_else(|_| "android-30".to_string());
        let api = platform
            .strip_prefix("android-")
            .unwrap_or(&platform)
            .to_string();

        cfg.define(
            "CMAKE_TOOLCHAIN_FILE",
            ndk.join("build/cmake/android.toolchain.cmake"),
        );
        cfg.define("ANDROID_ABI", android_abi(&target));
        cfg.define("ANDROID_PLATFORM", &platform);
        cfg.define("ANDROID_STL", "c++_shared");
        // ggml-vulkan includes <vulkan/vulkan.hpp> (the Vulkan-Hpp C++ bindings), which the
        // NDK sysroot does not ship (it only has the C header vulkan/vulkan.h). Staging
        // vulkan.hpp alone is not enough either: it pulls in vulkan_hpp_macros.hpp,
        // vulkan_enums.hpp, ... So CI stages a matched Vulkan-Headers + Vulkan-Hpp snapshot
        // and exports VULKAN_HEADERS_INCLUDE / VULKAN_HPP_INCLUDE.
        //
        // The two halves must come from the same release. ggml-vulkan-types.h selects the
        // dispatcher namespace from VK_HEADER_VERSION (>= 301 -> vk::detail::DispatchLoaderDynamic,
        // below -> vk::DispatchLoaderDynamic), so pairing a new vulkan.hpp with the NDK's older
        // vulkan.h takes the wrong branch and fails to compile. Using both halves of one
        // snapshot makes VK_HEADER_VERSION the value we chose (313) rather than whatever the
        // NDK happens to have.
        //
        // The C headers go to both compilers so C and C++ see the same Vulkan version; the C++
        // bindings are C++-only, so they can never leak into C translation units.
        if let Some(dir) = first_env(&["VULKAN_HEADERS_INCLUDE"]) {
            cfg.cflag(format!("-I{dir}"));
            cfg.cxxflag(format!("-I{dir}"));
        }
        if let Some(dir) = first_env(&["VULKAN_HPP_INCLUDE"]) {
            cfg.cxxflag(format!("-I{dir}"));
        }
        // ggml-vulkan-types.h also includes <spirv/unified1/spirv.hpp>, guarded by
        // __has_include with a plain #include as the fallback branch. The distro package has
        // it under /usr/include, but --sysroot hides /usr/include when cross-compiling, so
        // CI stages SPIRV-Headers too and exports SPIRV_HEADERS_INCLUDE.
        if let Some(dir) = first_env(&["SPIRV_HEADERS_INCLUDE"]) {
            cfg.cflag(format!("-I{dir}"));
            cfg.cxxflag(format!("-I{dir}"));
        }
        // ggml-vulkan calls `find_package(SPIRV-Headers CONFIG REQUIRED)`, but never actually
        // uses the package afterwards (no include dirs, no linked target). Under the NDK
        // cross-compile toolchain find_package is restricted to the sysroot and cannot see the
        // host's SPIRV-Headers, so configure dies with "Could not find a package configuration
        // file provided by SPIRV-Headers". Point it at a generated stub config; if a real
        // installation should be used instead, set SPIRV_HEADERS_DIR to its cmake dir.
        let spirv_dir = match first_env(&["SPIRV_HEADERS_DIR"]) {
            Some(dir) => PathBuf::from(dir),
            None => {
                let stub = PathBuf::from(env::var("OUT_DIR").unwrap()).join("spirv-headers-cmake");
                std::fs::create_dir_all(&stub).expect("failed to create SPIRV-Headers stub dir");
                std::fs::write(
                    stub.join("SPIRV-HeadersConfig.cmake"),
                    "set(SPIRV-Headers_FOUND TRUE)\n",
                )
                .expect("failed to write SPIRV-Headers stub config");
                stub
            }
        };
        cfg.define("SPIRV-Headers_DIR", spirv_dir.display().to_string());

        let prebuilt = ndk
            .join("toolchains/llvm/prebuilt")
            .join(ndk_host_tag(&host));
        if !prebuilt.exists() {
            panic!(
                "NDK at {} has no prebuilt toolchain for host {host} (looked in {})",
                ndk.display(),
                prebuilt.display()
            );
        }
        let sysroot = prebuilt.join("sysroot");
        android = Some((sysroot, android_clang_triple(&target, &api)));
    }

    // llama.cpp's ggml uses <filesystem>, which Apple's libc++ only exposes
    // from macOS 10.15 up. Without an explicit floor, cc-rs falls back to the
    // Xcode SDK's DefaultDeploymentTarget (e.g. 10.13) and the compile dies
    // with "'path' is unavailable: introduced in macOS 10.15".
    if is_macos {
        // IMPORTANT: never read MACOSX_DEPLOYMENT_TARGET here!
        // The macOS runner image already sets it to 10.13, and a value exported by the
        // workflow may still read as 10.13 inside the build-script process, forcing the
        // deployment target back to 10.13. So we use a dedicated knob, LLAMA_MACOS_MIN_VERSION,
        // defaulting to 11.0 (first Apple Silicon macOS).
        let min_version =
            first_env(&["LLAMA_MACOS_MIN_VERSION"]).unwrap_or_else(|| "11.0".to_string());
        cfg.define("CMAKE_OSX_DEPLOYMENT_TARGET", &min_version);

        // cmake-rs injects cc's derived base flags into CMAKE_{C,CXX,ASM}_FLAGS, including
        // -mmacosx-version-min=<Xcode SDK default>; MACOSX_DEPLOYMENT_TARGET alone often
        // fails to override it. Once we define these vars ourselves, cmake-rs stops appending
        // its own, so we supply them fully here with the version baked into the target triple.
        let arch = if target.starts_with("aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        let common = format!(
            "-ffunction-sections -fdata-sections -fPIC \
             --target={}-apple-macosx{} -mmacosx-version-min={} -w",
            arch, min_version, min_version
        );
        cfg.define("CMAKE_C_FLAGS", &common);
        cfg.define("CMAKE_CXX_FLAGS", &common);
        cfg.define("CMAKE_ASM_FLAGS", &common);

        // The cc::Build that compiles shim.cpp below reads this; override it unconditionally.
        env::set_var("MACOSX_DEPLOYMENT_TARGET", &min_version);

        // ggml-metal's Objective-C file (ggml-metal-device.m) uses @available(macOS ...), so
        // clang emits a call to __isPlatformVersionAtLeast. rustc links macOS executables with
        // -nodefaultlibs by default, skipping clang's runtime lib, which then fails at link time
        // with "Undefined symbols for architecture arm64: ___isPlatformVersionAtLeast".
        // Add libclang_rt.osx.a (located via clang --print-runtime-dir, never hard-coded).
        if let Ok(out) = std::process::Command::new("clang").arg("--print-runtime-dir").output() {
            let rt = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !rt.is_empty() {
                println!("cargo:rustc-link-search=native={rt}");
                println!("cargo:rustc-link-lib=static=clang_rt.osx");
            }
        }
    }

    if is_ios {
        let min_version = first_env(&["IOS_MIN_VERSION", "IPHONEOS_DEPLOYMENT_TARGET"])
            .unwrap_or_else(|| "14.0".to_string());
        let sdk = env::var("SDKROOT").unwrap_or_else(|_| {
            if is_ios_sim {
                "iphonesimulator".to_string()
            } else {
                "iphoneos".to_string()
            }
        });

        cfg.define("CMAKE_SYSTEM_NAME", "iOS");
        cfg.define("CMAKE_OSX_SYSROOT", sdk);
        cfg.define("CMAKE_OSX_ARCHITECTURES", ios_arch(&target));
        cfg.define("CMAKE_OSX_DEPLOYMENT_TARGET", &min_version);

        // cc (the crate compiling shim.cpp) reads this for -miphoneos-version-min.
        if env::var_os("IPHONEOS_DEPLOYMENT_TARGET").is_none() {
            env::set_var("IPHONEOS_DEPLOYMENT_TARGET", &min_version);
        }
    }

    let dst = cfg.build();
    let build_dir = dst.join("build");

    // ---- C++ shim over llama.cpp's official common/mtmd layers ----
    let mut ccfg = cc::Build::new();
    ccfg.cpp(true)
        .file(manifest_dir.join("shim/shim.cpp"))
        .include(llama_dir.join("include"))
        .include(llama_dir.join("ggml/include"))
        .include(llama_dir.join("common"))
        .include(llama_dir.join("vendor"))
        .include(llama_dir.join("tools/mtmd"))
        .include(&build_dir)
        .include(build_dir.join("common"))
        .std("c++17")
        .warnings(false);

    if let Some((sysroot, triple)) = &android {
        let clang = sysroot
            .parent() // .../toolchains/llvm/prebuilt/<tag>
            .expect("sysroot has a parent")
            .join("bin")
            .join(if is_windows { "clang++.exe" } else { "clang++" });
        ccfg.compiler(clang);
        ccfg.flag(&format!("--target={triple}"));
        ccfg.flag(&format!("--sysroot={}", sysroot.display()));
    } else if is_apple {
        ccfg.flag("-std=c++17");
    }
    ccfg.compile("shim");

    // ---- link ----
    let lib_dir = if dst.join("lib64").exists() {
        dst.join("lib64")
    } else {
        dst.join("lib")
    };
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    // vendor libs aren't installed; link straight from the build tree
    println!(
        "cargo:rustc-link-search=native={}",
        build_dir.join("vendor/hash").display()
    );
    if is_windows && is_msvc {
        // Visual Studio is a multi-config generator, so it appends $<CONFIG> to the archive
        // output directory: vendor-hash.lib lands in <build>/vendor/hash/Release/, whereas
        // the Makefile/Ninja generators used on Linux/macOS put it straight in
        // <build>/vendor/hash/. Without this extra search path the link dies with
        // "could not find native static library `vendor-hash`, perhaps an -L flag is missing?".
        // (Everything else is linked from the install prefix, which has no config subdir.)
        println!(
            "cargo:rustc-link-search=native={}",
            build_dir.join("vendor/hash/Release").display()
        );
    }
    // llama.cpp compiles common/build-info.cpp into its own static library, llama-common-base,
    // and never installs it - it only ever exists inside the build tree. It owns
    // llama_build_number() / llama_commit() / llama_compiler() / llama_build_target(), and
    // llama-common's common.obj references all four. On Windows MSVC that surfaces as
    //   LNK2019: unresolved external symbol "int __cdecl llama_build_number(void)"  (x4)
    //   LNK1120: 4 unresolved externals
    // because nothing on the link line provides them.
    let common_dir = build_dir.join("common");
    println!("cargo:rustc-link-search=native={}", common_dir.display());
    if is_windows && is_msvc {
        // Same multi-config quirk as vendor-hash above.
        println!(
            "cargo:rustc-link-search=native={}",
            common_dir.join("Release").display()
        );
    }
    if let Some((sysroot, triple)) = &android {
        // NDK's C++ runtime / libm live here and are not on rustc's default path
        println!(
            "cargo:rustc-link-search=native={}",
            sysroot.join("usr/lib").join(triple).display()
        );
    }

    // order matters for static archives: consumers before providers
    let mut libs = vec![
        "llama-common",
        "mtmd",
        "llama",
        "ggml",
        "ggml-base",
        "ggml-cpu",
    ];
    // llama-common-base (see the link-search note above) is only linked when CMake actually
    // produced it. Platforms whose linker never pulls common.obj in resolve fine without it,
    // and forcing it there would risk a "native static library not found" error instead.
    if has_static_lib(&common_dir, "llama-common-base")
        || has_static_lib(&common_dir.join("Release"), "llama-common-base")
    {
        libs.push("llama-common-base");
    }
    libs.push("vendor-hash");
    match backend.as_str() {
        "metal" => libs.push("ggml-metal"),
        "vulkan" => libs.push("ggml-vulkan"),
        "opencl" => libs.push("ggml-opencl"),
        _ => {}
    }
    for lib in libs {
        println!("cargo:rustc-link-lib=static={lib}");
    }

    // ---- platform runtime / system libs ----
    if is_apple {
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=MetalKit");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=QuartzCore");
        // ggml-cpu uses -mcpu=applesilicon with vDSP to accelerate *_vadd/_vsmul/...;
        // without this framework the final link fails with
        // "ld: symbol(s) not found for architecture arm64".
        println!("cargo:rustc-link-lib=framework=Accelerate");
        println!("cargo:rustc-link-lib=dylib=c++");
    } else if is_android {
        // Android has no libstdc++; Rust's Android std uses the shared libc++
        // from the NDK, so link the same one to avoid mixing two runtimes.
        println!("cargo:rustc-link-lib=c++_shared");
    } else if !is_windows || !is_msvc {
        // Linux and MinGW pull in the C++ runtime explicitly; MSVC does it itself.
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }

    if backend == "vulkan" && !is_apple {
        // ggml-vulkan links the Vulkan loader dynamically at runtime.
        // Windows imports from vulkan-1.dll/.lib, everything else uses -lvulkan.
        if is_windows {
            // The import library lives in the Vulkan SDK, which is not on rustc's default
            // search path, so the final link dies with
            //   "LINK : fatal error LNK1181: cannot open input file 'vulkan-1.lib'".
            // The SDK layout is <sdk>/Lib/vulkan-1.lib; the lowercase variant is added too so
            // a differently laid-out SDK still resolves (a missing -L directory is harmless).
            if let Some(sdk) = first_env(&["VULKAN_SDK"]) {
                let sdk = strip_verbatim(PathBuf::from(sdk));
                println!("cargo:rustc-link-search=native={}", sdk.join("Lib").display());
                println!("cargo:rustc-link-search=native={}", sdk.join("lib").display());
            }
            println!("cargo:rustc-link-lib=vulkan-1");
        } else {
            println!("cargo:rustc-link-lib=dylib=vulkan");
        }
    }

    // ---- bindgen for the raw C APIs still used directly (gguf probe) ----
    // Cross builds parse the headers with the host clang; llama.h only needs
    // plain libc types and every supported target is LP64, so the layout matches.
    // If a toolchain ever disagrees, override per target with
    // BINDGEN_EXTRA_CLANG_ARGS_<TARGET> (clang-sys picks it up automatically).
    // For Android cross-compilation, clang must be given the target triple and NDK sysroot
    // explicitly, otherwise it searches the host's headers and fails with
    // "/usr/include/stdint.h:26:10: fatal error: 'bits/libc-header-start.h' file not found".
    let mut bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", llama_dir.join("include").display()))
        .clang_arg(format!("-I{}", llama_dir.join("ggml/include").display()))
        .clang_arg(format!("-I{}", llama_dir.join("tools/mtmd").display()));

    if let Some((sysroot, triple)) = &android {
        bindings = bindings
            .clang_arg(format!("--target={triple}"))
            .clang_arg(format!("--sysroot={}", sysroot.display()));
    }

    let bindings = bindings
        .allowlist_function("llama_.*")
        .allowlist_function("mtmd_.*")
        .allowlist_function("gguf_.*")
        .allowlist_type("llama_.*")
        .allowlist_type("mtmd_.*")
        .allowlist_type("gguf_.*")
        .allowlist_var("llama_.*")
        .allowlist_var("mtmd_.*")
        .allowlist_var("GGUF_.*")
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: false,
        })
        .generate_comments(false)
        .generate()
        .expect("bindgen failed to generate llama.cpp bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write bindings.rs");
}