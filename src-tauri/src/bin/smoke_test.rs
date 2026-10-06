//! Engine smoke test: load a GGUF, optionally an mmproj + image, and
//! run one short generation. Usage:
//!   smoke_test <model.gguf> [mmproj.gguf] [image]
use etet_preview_lib::engine::{self, ChatMsg, LoadParams};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = args.get(1).expect("usage: smoke_test <model.gguf> [mmproj.gguf] [image]");

    println!("[smoke] loading {model} ...");
    let res = engine::load(
        LoadParams {
            model_path: model.clone(),
            mmproj_path: args.get(2).cloned(),
            n_ctx: 4096,
            n_gpu_layers: 0,
            flash_attn: false,
        },
        &|stage| println!("[smoke] {stage}"),
    )
    .expect("load failed");
    println!(
        "[smoke] loaded: {} ctx={} params={} vision={}",
        res.model_name, res.n_ctx, res.n_params, res.has_vision
    );

    // optional image -> base64 payload for the vision path
    let mut images: Vec<String> = Vec::new();
    if let Some(img_path) = args.get(3) {
        let bytes = std::fs::read(img_path).expect("failed to read image");
        use base64::Engine;
        images.push(base64::engine::general_purpose::STANDARD.encode(&bytes));
        println!("[smoke] image: {} ({} bytes)", img_path, bytes.len());
    }

    let prompt = if images.is_empty() {
        "1+1=? Answer with just the number.".to_string()
    } else {
        "Describe this image in one sentence.".to_string()
    };
    let msgs = vec![ChatMsg {
        role: "user".into(),
        content: prompt,
    }];
    let t0 = std::time::Instant::now();
    engine::generate(
        &msgs,
        &images,
        128,
        true,
        &|reasoning, _reset, piece| {
            if reasoning {
                print!("<think>{piece}</think>");
            } else {
                print!("{piece}");
            }
        },
        &|stopped| {
            println!("\n[smoke] done (stopped={stopped}) in {:.1?} OK", t0.elapsed());
        },
    )
    .expect("generation failed");
}
