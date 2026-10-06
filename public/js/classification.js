/* ============================================================
 * ETET Preview - classification.js
 * Display-side classification of RAW model output, per architecture.
 *
 * The output stream from the engine is NEVER modified; this module only
 * decides what RENDERS in the think bubble vs the body bubble.
 *
 * Rules (for architectures in ARCH_THINK only):
 *   - a think region runs from the start tag to the end tag; everything
 *     inside (leaked markers included) renders in the think bubble
 *   - stray llama-style special tokens (<|...|>) fold into think too
 *   - body text renders normally; a direct answer with no tags is body
 *
 * Architectures NOT in ARCH_THINK get no think bubble (all body).
 * To support a new architecture: add one ARCH_THINK entry. Tag pairs come
 * from the official llama.cpp parser sources (common/parsers/*.cpp).
 * ============================================================ */

const Classification = (() => {
    "use strict";

    // ---- architecture → reasoning tag pairs ----
    // arch entries are lowercase prefixes of the GGUF architecture string.
    const THINK_TAG = ["<think>", "</think>"];
    const ARCH_THINK = [
        { arch: ["qwen"],                          pairs: [THINK_TAG] }, // qwen/qwen2/qwen3/qwen35/moe/vl/qwen4exp
        { arch: ["deepseek"],                      pairs: [THINK_TAG] }, // deepseek/deepseek2/32/4
        { arch: ["etetmoe"],                       pairs: [THINK_TAG] }, // ETET (etetmoe-llama)
        { arch: ["glm4"],                          pairs: [THINK_TAG] }, // GLM-4 / GLM-4.5
        { arch: ["minicpm"],                       pairs: [THINK_TAG] }, // MiniCPM
        { arch: ["minimax"],                       pairs: [THINK_TAG] }, // MiniMax-01/M2/M3
        { arch: ["kimi"],                          pairs: [THINK_TAG] }, // Kimi
        { arch: ["smallthinker"],                  pairs: [THINK_TAG] },
        { arch: ["dots"],                          pairs: [THINK_TAG] }, // dots1/dots3
        { arch: ["bailingmoe"],                    pairs: [THINK_TAG] },
        { arch: ["ernie"],                         pairs: [THINK_TAG] },
        { arch: ["afmoe"],                         pairs: [THINK_TAG] },
        { arch: ["grovemoe"],                      pairs: [THINK_TAG] },
        { arch: ["exaone"],                        pairs: [THINK_TAG] },
        { arch: ["mimo"],                          pairs: [THINK_TAG] },
        { arch: ["mistral3", "mistral4"],          pairs: [["[THINK]", "[/THINK]"]] }, // Magistral
        { arch: ["gemma"],                         pairs: [["<|channel>thought", "<channel|>"]] }, // Gemma thought channel
        { arch: ["gpt_oss"],                       pairs: [["<|channel|>analysis<|message|>", "<|end|>"]] },
        // llama / granite / olmo / stablelm / bloom … → no reasoning tags:
        // leave them out (or add with an empty pairs list) → no think bubble.
    ];

    // any llama-style special token is a marker (never body prose), with or
    // without the trailing pipe: <|end|>, <|channel>, <|message|> …
    const SPECIAL_TOKEN_RE = /<\|[^<>|]{0,64}(\|>)?>/g;

    function resolvePairs(arch) {
        if (!arch) return [];
        const a = String(arch).toLowerCase();
        for (const e of ARCH_THINK) {
            if (e.arch.some((s) => a.startsWith(s))) return e.pairs;
        }
        return [];
    }

    /**
     * Split raw text into {think, body}. Think segments keep the markers.
     * Whole-text rescan: safe to call again on every streaming token.
     */
    function split(raw, arch) {
        if (!raw) return { think: "", body: "" };
        const pairs = resolvePairs(arch);
        if (pairs.length === 0) return { think: "", body: raw };

        let think = "";
        let body = "";
        let pos = 0;
        while (pos < raw.length) {
            let evPos = -1;
            let evPair = null;
            let evSpecial = null;
            for (const p of pairs) {
                const idx = raw.indexOf(p[0], pos);
                // pair wins ties against the bare special token it contains
                if (idx >= 0 && (evPos < 0 || idx <= evPos)) { evPos = idx; evPair = p; evSpecial = null; }
            }
            SPECIAL_TOKEN_RE.lastIndex = pos;
            const sp = SPECIAL_TOKEN_RE.exec(raw);
            if (sp && (evPos < 0 || sp.index < evPos)) { evPos = sp.index; evPair = null; evSpecial = sp[0]; }
            if (evPos < 0) { body += raw.slice(pos); break; }
            body += raw.slice(pos, evPos);
            if (evPair) {
                const end = raw.indexOf(evPair[1], evPos + evPair[0].length);
                if (end < 0) { think += raw.slice(evPos); pos = raw.length; }
                else { think += raw.slice(evPos, end + evPair[1].length); pos = end + evPair[1].length; }
            } else {
                think += evSpecial;
                pos = evPos + evSpecial.length;
            }
        }
        return { think, body };
    }

    /**
     * Display variant: holds back a trailing half-formed marker (e.g. "<thi",
     * "[THIN") so it doesn't flash in the body before it completes. Regular
     * text with a bare "<" or "[" is not held back.
     */
    function splitForDisplay(raw, arch) {
        if (!raw) return { think: "", body: "" };
        let hold = 0;
        const lt = raw.lastIndexOf("<");
        if (lt >= 0) {
            const tail = raw.slice(lt);
            if (tail.length <= 32 && /^<\/?[a-zA-Z!|][^>]*$/.test(tail)) hold = tail.length;
        }
        if (hold === 0) {
            const lb = raw.lastIndexOf("[");
            if (lb >= 0) {
                const tail = raw.slice(lb);
                if (tail.length <= 16 && /^\[[A-Z\/]{1,14}$/.test(tail)) hold = tail.length;
            }
        }
        return split(raw.slice(0, raw.length - hold), arch);
    }

    return { split, splitForDisplay };
})();
