// shim.cpp — C ABI bridge over llama.cpp's OFFICIAL layers.
// Chat templates (jinja), tokenization, multimodal eval, sampling, stop
// handling and output parsing all come from llama.cpp's own common/ and
// tools/mtmd code paths (the same ones llama-server / llama-cli use).
// No model-specific logic lives here — everything is driven by the GGUF.
#include "common.h"
#include "chat.h"
#include "sampling.h"

#include "llama.h"
#include "mtmd.h"
#include "mtmd-helper.h"

#include <cstdlib>
#include <cstring>
#include <algorithm>
#include <string>
#include <thread>
#include <vector>

struct shim_engine {
    llama_model * model = nullptr;
    llama_context * ctx = nullptr;
    mtmd_context * mctx = nullptr; // null when no mmproj
    const llama_vocab * vocab = nullptr; // not owned
    common_chat_templates_ptr tmpls;
    bool has_vision = false;
    uint32_t n_ctx = 0;
};

extern "C" {

typedef void (*shim_progress_cb)(const char * stage, void * user);
typedef void (*shim_delta_cb)(void * user, int is_reasoning, int reset, const char * text);
typedef int (*shim_stop_poll)(void * user); // returns 1 to abort generation

shim_engine * shim_engine_init(const char * model_path,
                               const char * mmproj_path,
                               int n_ctx,
                               int n_gpu_layers,
                               int flash_attn,
                               shim_progress_cb cb,
                               void * cb_user) {
    auto progress = [&](const char * s) {
        if (cb) {
            cb(s, cb_user);
        }
    };

    progress("loading model");
    llama_model_params mp = llama_model_default_params();
    mp.n_gpu_layers = n_gpu_layers;
    llama_model * model = llama_model_load_from_file(model_path, mp);
    if (!model) {
        return nullptr;
    }

    progress("creating context");
    llama_context_params cp = llama_context_default_params();
    cp.n_ctx = (uint32_t) n_ctx;
    cp.n_batch  = 1024;
    cp.n_ubatch = 1024;
    cp.flash_attn_type = flash_attn ? LLAMA_FLASH_ATTN_TYPE_ENABLED
                                    : LLAMA_FLASH_ATTN_TYPE_DISABLED;
    cp.n_threads = std::thread::hardware_concurrency();
    cp.n_threads_batch = cp.n_threads;
    llama_context * ctx = llama_init_from_model(model, cp);
    if (!ctx) {
        llama_model_free(model);
        return nullptr;
    }

    mtmd_context * mctx = nullptr;
    if (mmproj_path && mmproj_path[0]) {
        progress("loading vision projector");
        mtmd_context_params mparams = mtmd_context_params_default();
        mparams.use_gpu = true;
        mctx = mtmd_init_from_file(mmproj_path, model, mparams);
        if (!mctx) {
            llama_free(ctx);
            llama_model_free(model);
            return nullptr;
        }
    }

    auto * e = new shim_engine();
    e->model = model;
    e->ctx = ctx;
    e->mctx = mctx;
    e->vocab = llama_model_get_vocab(model);
    e->has_vision = mctx && mtmd_support_vision(mctx);
    e->n_ctx = llama_n_ctx(ctx);
    e->tmpls = common_chat_templates_init(model, "");
    return e;
}

void shim_engine_free(shim_engine * e) {
    if (!e) {
        return;
    }
    if (e->mctx) {
        mtmd_free(e->mctx);
    }
    llama_free(e->ctx);
    llama_model_free(e->model);
    delete e;
}

bool shim_has_vision(shim_engine * e) {
    return e && e->has_vision;
}

// length of the longest complete UTF-8 prefix of s
static size_t utf8_valid_len(const std::string & s) {
    size_t i = s.size();
    while (i > 0) {
        unsigned char c = (unsigned char) s[i - 1];
        if ((c & 0xC0) != 0x80) { // start byte found
            break;
        }
        i--;
    }
    if (i == 0) {
        return s.size(); // pure ASCII tail
    }
    unsigned char c = (unsigned char) s[i - 1];
    size_t need = 1;
    if      ((c & 0xE0) == 0xC0) need = 2;
    else if ((c & 0xF0) == 0xE0) need = 3;
    else if ((c & 0xF8) == 0xF0) need = 4;
    if (s.size() - (i - 1) >= need) {
        return s.size(); // last sequence complete
    }
    return i - 1; // hold back the incomplete tail
}

int shim_generate(shim_engine * e,
                  const char * const * roles,
                  const char * const * contents,
                  size_t n_msgs,
                  const unsigned char * const * images,
                  const size_t * image_lens,
                  size_t n_images,
                  int max_tokens,
                  int enable_thinking,
                  shim_delta_cb on_delta,
                  void * delta_user,
                  shim_stop_poll stop_poll,
                  void * stop_user) {
    if (!e || !e->tmpls) {
        return -1;
    }

    llama_memory_clear(llama_get_memory(e->ctx), true);

    // ---- 1. messages; media marker trails the last user turn ----
    const char * marker = e->mctx ? mtmd_get_marker(e->mctx) : nullptr;
    std::vector<common_chat_msg> msgs;
    msgs.reserve(n_msgs);
    for (size_t i = 0; i < n_msgs; i++) {
        common_chat_msg m;
        m.role = roles[i];
        m.content = contents[i];
        if (e->has_vision && marker && i + 1 == n_msgs && std::strcmp(roles[i], "user") == 0) {
            m.content += "\n";
            m.content += marker;
        }
        msgs.push_back(std::move(m));
    }

    // ---- 2. chat template (jinja, model's own template) ----
    common_chat_templates_inputs ti;
    ti.messages = msgs;
    ti.add_generation_prompt = true;
    ti.use_jinja = true;
    ti.enable_thinking = enable_thinking != 0;
    ti.reasoning_format = COMMON_REASONING_FORMAT_AUTO;
    common_chat_params cp = common_chat_templates_apply(e->tmpls.get(), ti);

    // ---- 3. tokenize + eval (official mtmd path when images present) ----
    llama_pos n_past = 0;
    std::vector<llama_token> prompt_tokens;
    if (n_images > 0 && e->mctx) {
        std::vector<mtmd_bitmap *> bitmaps;
        for (size_t i = 0; i < n_images; i++) {
            auto w = mtmd_helper_bitmap_init_from_buf(e->mctx, images[i], image_lens[i],
                                                      /*placeholder=*/false,
                                                      mtmd_helper_init_opt_default());
            if (!w.bitmap) {
                return -2;
            }
            bitmaps.push_back(w.bitmap);
        }
        mtmd_input_chunks * chunks = mtmd_input_chunks_init();
        mtmd_input_text txt{ cp.prompt.c_str(), cp.prompt.size(), /*add_special=*/false, /*parse_special=*/true };
        int rc = mtmd_tokenize(e->mctx, chunks, &txt, bitmaps.data(), bitmaps.size());
        for (auto * b : bitmaps) {
            mtmd_bitmap_free(b);
        }
        if (rc != 0) {
            mtmd_input_chunks_free(chunks);
            return -3;
        }
        int32_t erc = mtmd_helper_eval_chunks(e->mctx, e->ctx, chunks, 0, 0, 512, /*logits_last=*/true, &n_past);
        // feed text tokens to the sampler for penalty statistics
        size_t n_chunks = mtmd_input_chunks_size(chunks);
        for (size_t i = 0; i < n_chunks; i++) {
            const mtmd_input_chunk * c = mtmd_input_chunks_get(chunks, i);
            if (mtmd_input_chunk_get_type(c) == mtmd_input_chunk_type::MTMD_INPUT_CHUNK_TYPE_TEXT) {
                size_t n_tok = 0;
                const llama_token * toks = mtmd_input_chunk_get_tokens_text(c, &n_tok);
                for (size_t k = 0; k < n_tok; k++) {
                    prompt_tokens.push_back(toks[k]);
                }
            }
        }
        mtmd_input_chunks_free(chunks);
        if (erc != 0) {
            return -4;
        }
    } else {
        std::vector<llama_token> toks(cp.prompt.size() + 16);
        int n = llama_tokenize(e->vocab, cp.prompt.data(), (int32_t) cp.prompt.size(),
                               toks.data(), (int) toks.size(), /*add_special=*/true, /*parse_special=*/true);
        if (n < 0) {
            toks.resize((size_t) (-n));
            n = llama_tokenize(e->vocab, cp.prompt.data(), (int32_t) cp.prompt.size(),
                               toks.data(), (int) toks.size(), true, true);
        }
        if (n < 0) {
            return -5;
        }
        toks.resize((size_t) n);
        prompt_tokens = toks;
        for (size_t i = 0; i < toks.size(); i += 512) {
            size_t ntok = std::min<size_t>(512, toks.size() - i);
            llama_batch batch = llama_batch_get_one(&toks[i], (int) ntok);
            if (llama_decode(e->ctx, batch) != 0) {
                return -7;
            }
            n_past += (llama_pos) ntok;
        }
    }

    // ---- 4. sampler (official chain: penalties/DRY/top-k/typical/top-p/min-p/temp) ----
    common_params_sampling sp; // llama-cli defaults
    common_sampler * smpl = common_sampler_init(e->model, sp);
    for (llama_token t : prompt_tokens) {
        common_sampler_accept(smpl, t, /*accept_grammar=*/false);
    }

    // single-token stop forms (llama-cli matches these by token id)
    std::vector<llama_token> stop_tokens;
    for (const auto & s : cp.additional_stops) {
        llama_token tmp[32];
        int n = llama_tokenize(e->vocab, s.c_str(), (int32_t) s.size(), tmp, 32, false, true);
        if (n == 1) {
            stop_tokens.push_back(tmp[0]);
        }
    }


    common_chat_parser_params pp(cp);
    pp.reasoning_format = COMMON_REASONING_FORMAT_AUTO;

    std::string gen_text; // full generated text (bytes, complete UTF-8 only)
    std::string pending;  // incomplete UTF-8 tail
    std::string prev_reasoning, prev_content;
    bool stopped_by_user = false;
    int n_gen = 0;

    auto emit = [&]() {
        common_chat_msg msg = common_chat_parse(gen_text, /*is_partial=*/true, pp);
        const std::string & r = msg.reasoning_content;
        const std::string & c = msg.content;
        if (r.compare(0, prev_reasoning.size(), prev_reasoning) == 0) {
            if (r.size() > prev_reasoning.size()) {
                on_delta(delta_user, 1, 0, r.c_str() + prev_reasoning.size());
            }
        } else {
            on_delta(delta_user, 1, 1, r.c_str());
        }
        prev_reasoning = r;
        if (c.compare(0, prev_content.size(), prev_content) == 0) {
            if (c.size() > prev_content.size()) {
                on_delta(delta_user, 0, 0, c.c_str() + prev_content.size());
            }
        } else {
            on_delta(delta_user, 0, 1, c.c_str());
        }
        prev_content = c;
    };

    llama_token new_tok = common_sampler_sample(smpl, e->ctx, -1);
    while (true) {
        if (stop_poll && stop_poll(stop_user)) {
            stopped_by_user = true;
            break;
        }
        if (llama_vocab_is_eog(e->vocab, new_tok)) {
            break;
        }
        bool stop_hit = false;
        for (llama_token t : stop_tokens) {
            if (new_tok == t) {
                stop_hit = true;
                break;
            }
        }
        if (stop_hit) {
            break;
        }

        char piece[256];
        int plen = llama_token_to_piece(e->vocab, new_tok, piece, sizeof(piece), 0, true);
        if (plen > 0) {
            pending.append(piece, (size_t) plen);
            size_t ok = utf8_valid_len(pending);
            gen_text.append(pending, 0, ok);
            pending.erase(0, ok);
            // template-provided stop strings, scanned across token boundaries
            for (const auto & s : cp.additional_stops) {
                size_t min_start = gen_text.size() > (s.size() + (size_t) plen + 2)
                                       ? gen_text.size() - (s.size() + (size_t) plen + 2)
                                       : 0;
                size_t pos = gen_text.find(s, min_start);
                if (pos != std::string::npos) {
                    gen_text.resize(pos);
                    stop_hit = true;
                    break;
                }
            }
            emit();
        }
        common_sampler_accept(smpl, new_tok, /*accept_grammar=*/true);
        n_gen++;
        if (stop_hit || n_gen >= max_tokens || n_past >= (llama_pos) e->n_ctx - 2) {
            break;
        }

        llama_batch batch = llama_batch_get_one(&new_tok, 1);
        if (llama_decode(e->ctx, batch) != 0) {
            common_sampler_free(smpl);
            return -8;
        }
        new_tok = common_sampler_sample(smpl, e->ctx, -1);
    }

    gen_text += pending;
    pending.clear();
    emit();
    common_sampler_free(smpl);
    return stopped_by_user ? 1 : 0;
}

void shim_free(void * p) {
    free(p);
}

} // extern "C"
