/* ============================================================
 * ETET Preview - controller (thin client)
 * The webview only forwards inputs to Rust and renders results;
 * all computation (think parsing, file I/O, truncation) is in Rust.
 * Streaming updates touch only the last message node, never the
 * whole list. Everything lives in memory; nothing persists.
 * ============================================================ */

(function () {
    "use strict";

    // ---- state (memory only) ----
    let modelInfo = null;        // from pick_model
    let selectedMmproj = null;   // path or null
    let sysInfo = null;          // {total_mem, available_mem, gpu_backend}
    let modelLoaded = false;
    let loadResult = null;
    let generating = false;
    let webEnabled = false;
    let byok = null;             // {provider, api_key, url} session-only
    let attachedImages = [];     // {name, b64}
    let attachedFiles = [];      // {name, text}
    let messages = [];           // single session, in-memory
    let streamNode = null;       // refs to the streaming bubble (incremental DOM)
    let loadSizeText = "";       // "5.4 GB + mmproj" shown while loading

    // ---- DOM ----
    const $ = (id) => document.getElementById(id);
    const el = {};

    // Tauri globals (withGlobalTauri = true)
    const invoke = (...args) => window.__TAURI__.core.invoke(...args);
    const listen = (...args) => window.__TAURI__.event.listen(...args);

    const TEXT_EXTS = [
        "txt", "md", "markdown", "json", "csv", "log", "yaml", "yml", "xml",
        "py", "js", "ts", "rs", "c", "h", "cpp", "hpp", "java", "go", "sh",
        "html", "css", "sql", "toml", "ini", "conf",
    ];
    const IMAGE_EXTS = ["png", "jpg", "jpeg", "webp", "bmp"];

    document.addEventListener("DOMContentLoaded", init);

    function init() {
        I18N.apply();
        ["screen-model", "screen-config", "screen-chat",
         "model-meta", "meta-name", "meta-arch", "meta-layers", "meta-ctx",
         "meta-size", "meta-mmproj", "row-mmproj-list", "mmproj-list",
         "btn-pick-model", "btn-back-model", "btn-to-config",
         "cfg-ctx", "cfg-ctx-hint", "cfg-split", "cfg-unified",
         "cfg-ngl", "cfg-ngl-val", "bar-gpu", "bar-gpu-val",
         "bar-ram", "bar-ram-val", "bar-unified", "bar-unified-val",
         "cfg-ram", "cfg-gpu",
         "btn-back-config", "btn-start-chat", "cfg-status",
         "load-progress", "load-progress-text",
         "btn-switch-model", "btn-clear", "model-status", "btn-export",
         "chat-scroll", "chat-empty", "message-list",
         "btn-image", "btn-file",
         "chat-input", "btn-send", "btn-web",
         "attached-row", "byok-modal", "byok-x", "byok-provider",
         "byok-url", "byok-url-row", "byok-key", "byok-save", "byok-skip",
        ].forEach((id) => { el[id] = $(id); });

        // model screen
        el["btn-pick-model"].addEventListener("click", pickModel);
        el["btn-back-model"].addEventListener("click", resetModel);
        el["btn-to-config"].addEventListener("click", () => showScreen("screen-config"));
        // config screen
        el["btn-back-config"].addEventListener("click", () => showScreen("screen-model"));
        el["btn-start-chat"].addEventListener("click", startEngine);
        el["cfg-ngl"].addEventListener("input", () => {
            updateNglLabel();
            updateMemBars();
        });
        el["cfg-ctx"].addEventListener("input", validateCtx);
        el["cfg-ctx"].addEventListener("change", () => { validateCtx(); updateMemBars(); });
        // session-only language switch on the model screen
        document.querySelectorAll("#lang-switch button").forEach((btn) => {
            btn.addEventListener("click", () => {
                I18N.setLang(btn.dataset.lang);
                markLangButtons();
            });
        });
        document.addEventListener("etetlangchange", markLangButtons);
        markLangButtons();
        // chat screen
        el["btn-switch-model"].addEventListener("click", backToModel);
        el["btn-clear"].addEventListener("click", clearContext);
        el["btn-export"].addEventListener("click", exportHistory);
        el["btn-send"].addEventListener("click", onSend);
        el["btn-web"].addEventListener("click", toggleWeb);
        el["btn-image"].addEventListener("click", attachImages);
        el["btn-file"].addEventListener("click", attachFiles);
        el["chat-input"].addEventListener("keydown", (e) => {
            if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); onSend(); }
        });
        el["chat-input"].addEventListener("input", autoGrow);
        // byok modal
        el["byok-provider"].addEventListener("change", () => {
            el["byok-url-row"].hidden = el["byok-provider"].value !== "custom";
        });
        el["byok-save"].addEventListener("click", saveByok);
        el["byok-skip"].addEventListener("click", skipByok);
        el["byok-x"].addEventListener("click", () => el["byok-modal"].classList.remove("open"));

        // engine streaming events
        listen("token", (ev) => onToken(ev.payload));       // {reasoning, reset, text}
        listen("gen-done", (ev) => onGenDone(ev.payload));
        listen("engine-progress", (ev) => {
            const payload = String(ev.payload);
            if (payload.startsWith("t:")) {
                // elapsed-seconds ticker from the loader
                el["load-progress-text"].textContent =
                    I18N.t("config.loadingFile", { size: loadSizeText }) + " · " +
                    I18N.t("config.elapsed", { s: payload.slice(2) });
            } else {
                el["cfg-status"].textContent =
                    I18N.t("chat.engineProgress", { stage: ev.payload });
            }
        });

        updateEmpty();
    }

    /* ---------------- screens ---------------- */
    function markLangButtons() {
        document.querySelectorAll("#lang-switch button").forEach((btn) => {
            btn.classList.toggle("active", btn.dataset.lang === I18N.getLang());
        });
    }

    function showScreen(id) {
        ["screen-model", "screen-config", "screen-chat"].forEach((s) => {
            el[s].hidden = s !== id;
        });
        if (id === "screen-config") enterConfig();
    }

    /* ---------------- screen 1: model ---------------- */
    async function pickModel() {
        const picked = await window.__TAURI__.dialog.open({
            multiple: false,
            filters: [{ name: "GGUF", extensions: ["gguf"] }],
        });
        if (!picked) return;
        try {
            modelInfo = await invoke("pick_model", { path: picked });
        } catch (err) {
            showToast(I18N.t("setup.pickFail", { err: String(err) }), true);
            return;
        }
        selectedMmproj = modelInfo.mmproj_candidates.length
            ? modelInfo.mmproj_candidates[0].path
            : null;

        el["meta-name"].textContent = modelInfo.name || modelInfo.file_name;
        el["meta-arch"].textContent = modelInfo.arch;
        el["meta-layers"].textContent = modelInfo.n_layers;
        el["meta-ctx"].textContent = fmtTokens(modelInfo.n_ctx_train);
        el["meta-size"].textContent = fmtSize(modelInfo.size_bytes);
        el["meta-mmproj"].textContent = selectedMmproj
            ? I18N.t("setup.mmprojYes")
            : I18N.t("setup.mmprojNone");
        el["meta-mmproj"].style.color = selectedMmproj ? "#16ff98" : "#888";

        // mmproj candidates list (radio)
        const list = el["mmproj-list"];
        list.innerHTML = "";
        modelInfo.mmproj_candidates.forEach((c) => {
            const label = document.createElement("label");
            label.className = "mmproj-item" + (c.path === selectedMmproj ? " selected" : "");
            const radio = document.createElement("input");
            radio.type = "radio";
            radio.name = "mmproj";
            radio.checked = c.path === selectedMmproj;
            radio.addEventListener("change", () => {
                selectedMmproj = c.path;
                list.querySelectorAll(".mmproj-item").forEach((n) => n.classList.remove("selected"));
                label.classList.add("selected");
                el["meta-mmproj"].textContent = I18N.t("setup.mmprojYes");
                el["meta-mmproj"].style.color = "#16ff98";
            });
            label.appendChild(radio);
            label.appendChild(document.createTextNode(`${c.file_name} (${fmtSize(c.size_bytes)})`));
            list.appendChild(label);
        });
        el["row-mmproj-list"].hidden = modelInfo.mmproj_candidates.length < 2;

        el["model-meta"].hidden = false;
        el["btn-to-config"].hidden = false;
        el["btn-back-model"].hidden = false;
        el["btn-pick-model"].textContent = I18N.t("setup.repick");
    }

    function resetModel() {
        modelInfo = null;
        selectedMmproj = null;
        el["model-meta"].hidden = true;
        el["btn-to-config"].hidden = true;
        el["btn-back-model"].hidden = true;
        el["btn-pick-model"].textContent = I18N.t("setup.pick");
    }

    /* ---------------- screen 2: config ---------------- */
    async function enterConfig() {
        if (!sysInfo) {
            sysInfo = await invoke("system_info");
            el["cfg-ram"].textContent =
                fmtSize(sysInfo.available_mem) + " / " + fmtSize(sysInfo.total_mem);
            el["cfg-gpu"].textContent = sysInfo.gpu_backend;
        }
        const unified = sysInfo.gpu_backend === "Metal";
        // per ep.md: unified-memory SoCs get a single memory bar, no split
        el["cfg-split"].hidden = unified;
        el["cfg-unified"].hidden = !unified;

        if (modelInfo) {
            el["cfg-ngl"].max = modelInfo.n_layers || 32;
            el["cfg-ngl"].value = el["cfg-ngl"].max;
            updateNglLabel();
            // context hint: model max, user input is clamped to it
            const maxK = Math.max(1, Math.round((modelInfo.n_ctx_train || 4096) / 1024));
            el["cfg-ctx-hint"].textContent = I18N.t("config.ctxMax", { k: maxK });
            validateCtx();
            updateMemBars();
        }
    }

    let lastValidCtxK = 4;
    function maxCtxK() {
        return Math.max(1, Math.round((modelInfo && modelInfo.n_ctx_train || 4096) / 1024));
    }
    function validateCtx() {
        const raw = el["cfg-ctx"].value.trim();
        if (!/^\d+$/.test(raw)) {
            el["cfg-ctx"].value = String(lastValidCtxK);
            return;
        }
        let k = parseInt(raw, 10);
        if (k < 1) k = 1;
        const maxK = maxCtxK();
        if (k > maxK) k = maxK;
        el["cfg-ctx"].value = String(k);
        lastValidCtxK = k;
    }
    function ctxTokens() {
        return Number(el["cfg-ctx"].value) * 1024;
    }

    function updateNglLabel() {
        const v = Number(el["cfg-ngl"].value);
        const max = Number(el["cfg-ngl"].max);
        el["cfg-ngl-val"].textContent = `${v}/${max}`;
        const pct = max > 0 ? (v / max) * 100 : 0;
        el["cfg-ngl"].style.setProperty("--fill", pct + "%");
    }

    function updateMemBars() {
        if (!modelInfo) return;
        const layers = modelInfo.n_layers || 32;
        const ctx = ctxTokens();
        // rough upper bound: weights + kv cache (2 bytes * head_dim 2048 per layer per token)
        const kvTotal = 2 * layers * ctx * 2048;
        const total = modelInfo.size_bytes + kvTotal;

        if (sysInfo && sysInfo.gpu_backend === "Metal") {
            el["bar-unified"].style.width = "100%";
            el["bar-unified-val"].textContent = "~ " + fmtSize(total);
            return;
        }
        const frac = Number(el["cfg-ngl"].value) / (layers || 1);
        const gpuBytes = total * frac;
        el["bar-gpu"].style.width = (frac * 100).toFixed(1) + "%";
        el["bar-gpu-val"].textContent = "~ " + fmtSize(gpuBytes);
        el["bar-ram"].style.width = ((1 - frac) * 100).toFixed(1) + "%";
        el["bar-ram-val"].textContent = "~ " + fmtSize(total - gpuBytes);
    }

    async function startEngine() {
        if (!modelInfo) return;
        el["btn-start-chat"].disabled = true;
        el["cfg-status"].textContent = I18N.t("chat.modelLoading");
        loadSizeText = fmtSize(modelInfo.size_bytes) +
            (selectedMmproj ? " + " + I18N.t("setup.mmproj") : "");
        el["load-progress"].hidden = false;
        el["load-progress-text"].textContent =
            I18N.t("config.loadingFile", { size: loadSizeText }) + " · " +
            I18N.t("config.elapsed", { s: 0 });
        try {
            loadResult = await invoke("load_model", {
                options: {
                    model_path: modelInfo.path,
                    mmproj_path: selectedMmproj,
                    n_ctx: ctxTokens(),
                    n_gpu_layers: Number(el["cfg-ngl"].value),
                    flash_attn: false,
                },
            });
            modelLoaded = true;
            el["model-status"].textContent = loadResult.model_name;
            // image button live iff a working vision projector is loaded
            el["btn-image"].classList.toggle("disabled", !loadResult.has_vision);
            el["cfg-status"].textContent = I18N.t("config.loadedOk");
            el["load-progress"].hidden = true;
            showScreen("screen-chat");
            showToast(loadResult.model_name + " · " + I18N.t("config.loadedOk"));
        } catch (err) {
            showToast(I18N.t("config.loadFail", { err: String(err) }), true);
            el["cfg-status"].textContent = "";
            el["load-progress"].hidden = true;
        } finally {
            el["btn-start-chat"].disabled = false;
        }
    }

    async function backToModel() {
        if (generating) { showToast(I18N.t("chat.stop"), true); return; }
        await invoke("unload_model");
        modelLoaded = false;
        loadResult = null;
        messages = [];
        el["model-status"].textContent = I18N.t("chat.noModel");
        renderMessages();
        showScreen("screen-model");
    }

    /* ---------------- clear context ---------------- */
    async function clearContext() {
        if (!messages.length) return;
        if (generating) { await invoke("stop_generation"); generating = false; el["btn-send"].textContent = I18N.t("chat.send"); }
        const ok = await showConfirm("chat.clearConfirmTitle", "chat.clearConfirmBody");
        if (!ok) return;
        messages = [];
        streamNode = null;
        renderMessages();
        updateEmpty();
    }

    /* ---------------- BYOK ---------------- */
    function toggleWeb() {
        webEnabled = !webEnabled;
        el["btn-web"].classList.toggle("on", webEnabled);
        if (webEnabled && !byok) el["byok-modal"].classList.add("open");
    }

    function saveByok() {
        byok = {
            provider: el["byok-provider"].value,
            api_key: el["byok-key"].value,
            url: el["byok-url"].value || null,
        };
        el["byok-modal"].classList.remove("open");
        showToast(I18N.t("byok.saved"));
    }

    function skipByok() {
        byok = null;
        webEnabled = false;
        el["btn-web"].classList.remove("on");
        el["byok-modal"].classList.remove("open");
        showToast(I18N.t("byok.cleared"));
    }

    /* ---------------- attachments (Rust does all I/O) ---------------- */
    async function attachImages() {
        const paths = await window.__TAURI__.dialog.open({
            multiple: true,
            filters: [{ name: "Image", extensions: IMAGE_EXTS }],
        });
        if (!paths) return;
        for (const p of [].concat(paths)) {
            try {
                const img = await invoke("read_file_base64", { path: p });
                attachedImages.push({ name: img.file_name, b64: img.b64 });
            } catch (err) {
                showToast(String(err), true);
            }
        }
        renderAttached();
    }

    async function attachFiles() {
        const paths = await window.__TAURI__.dialog.open({
            multiple: true,
            filters: [{ name: "Text", extensions: TEXT_EXTS }],
        });
        if (!paths) return;
        for (const p of [].concat(paths)) {
            try {
                const text = await invoke("read_file_text", { path: p });
                attachedFiles.push({ name: p.split("/").pop().split("\\").pop(), text });
            } catch (err) {
                showToast(String(err), true);
            }
        }
        renderAttached();
    }

    function renderAttached() {
        el["attached-row"].innerHTML = "";
        attachedImages.forEach((a, i) => {
            el["attached-row"].appendChild(makeChip(
                I18N.t("chat.imageAttached") + " " + a.name,
                () => { attachedImages.splice(i, 1); renderAttached(); }));
        });
        attachedFiles.forEach((a, i) => {
            el["attached-row"].appendChild(makeChip(
                I18N.t("chat.fileAttached") + " " + a.name,
                () => { attachedFiles.splice(i, 1); renderAttached(); }));
        });
    }

    function makeChip(label, onRemove) {
        const chip = document.createElement("span");
        chip.className = "attached-chip";
        chip.textContent = label;
        const x = document.createElement("button");
        x.textContent = "×";
        x.onclick = onRemove;
        chip.appendChild(x);
        return chip;
    }

    /* ---------------- send / generate ---------------- */
    async function onSend() {
        if (!modelLoaded) { showToast(I18N.t("chat.selectModelFirst"), true); return; }
        if (generating) { await invoke("stop_generation"); return; }
        const text = el["chat-input"].value.trim();
        if (!text && attachedImages.length === 0) return;

        // BYOK web search: search first, inject results as context (Rust does the search)
        let webNote = "";
        if (webEnabled && byok && text) {
            el["model-status"].textContent = I18N.t("chat.webSearching");
            try {
                const results = await invoke("web_search", {
                    options: {
                        provider: byok.provider,
                        api_key: byok.api_key,
                        url: byok.url,
                        query: text,
                    },
                });
                if (results.length) {
                    webNote = results
                        .map((r, i) => `[${i + 1}] ${r.title}\n${r.url}\n${r.snippet}`)
                        .join("\n\n");
                    showToast(I18N.t("chat.webInjected", { n: results.length }));
                }
            } catch (err) {
                showToast(I18N.t("chat.webFailed", { err: String(err) }), true);
            }
            el["model-status"].textContent = loadResult.model_name;
        }

        // file contents are injected before the user text; the bubble only
        // shows the original text (m.display)
        let content = text;
        if (attachedFiles.length) {
            content = attachedFiles
                .map((a) => `--- ${a.name} ---\n${a.text}\n--- end ---`)
                .join("\n\n") + "\n\n" + text;
        }
        if (webNote) {
            content = `${content}\n\n--- Web search results ---\n${webNote}`;
        }

        messages.push({
            role: "user",
            content,
            display: text,
            imageNames: attachedImages.map((a) => a.name),
            fileNames: attachedFiles.map((a) => a.name),
        });

        const imagesB64 = attachedImages.map((a) => a.b64);
        el["chat-input"].value = "";
        autoGrow();
        attachedImages = [];
        attachedFiles = [];
        renderAttached();

        messages.push({
            role: "assistant",
            content: "",
            streaming: true,
        });
        renderMessages();
        streamNode = captureStreamingNode();
        scrollBottom();

        generating = true;
        el["btn-send"].textContent = I18N.t("chat.stop");

        try {
            await invoke("generate", {
                options: {
                    messages: messages
                        .filter((m) => m.role === "user" || !m.streaming)
                        .map((m) => ({ role: m.role, content: m.content })),
                    images_b64: imagesB64,
                    max_tokens: 1024,
                    // 模板引擎只对支持思考的模板生效，不支持的模型自动忽略
                    enable_thinking: true,
                },
            });
        } catch (err) {
            showToast(String(err), true);
            generating = false;
            el["btn-send"].textContent = I18N.t("chat.send");
        }
        // normal completion finalizes via the gen-done event
    }

    function onToken(delta) {
        const m = messages[messages.length - 1];
        if (!m || m.role !== "assistant") return;
        if (delta.reset) m.content = delta.text;
        else m.content += delta.text;
        // incremental DOM update: only the streaming bubbles, never a re-render
        if (streamNode) {
            const parts = Classification.splitForDisplay(m.content, modelInfo && modelInfo.arch);
            if (parts.think) {
                if (!streamNode.think) {
                    const last = streamNode.bubble.closest(".msg");
                    if (last) {
                        const tb = document.createElement("div");
                        tb.className = "think-block";
                        const lbl = document.createElement("span");
                        lbl.className = "think-label";
                        lbl.textContent = I18N.t("chat.thinkingBlock");
                        tb.appendChild(lbl);
                        tb.appendChild(document.createTextNode(""));
                        last.insertBefore(tb, streamNode.bubble);
                        streamNode.think = tb;
                    }
                }
                if (streamNode.think) {
                    streamNode.think.lastChild.textContent = parts.think;
                }
            }
            streamNode.bubble.textContent = parts.body || "…";
        }
        scrollBottom();
    }

    function onGenDone() {
        const m = messages[messages.length - 1];
        if (m && m.role === "assistant") m.streaming = false;
        generating = false;
        el["btn-send"].textContent = I18N.t("chat.send");
        streamNode = null;
        renderMessages();
        scrollBottom();
    }
    /* ---------------- rendering ---------------- */
    function renderMessages() {
        el["message-list"].innerHTML = "";
        streamNode = null;
        if (messages.length === 0) { updateEmpty(); return; }
        el["chat-empty"].style.display = "none";
        for (const m of messages) {
            el["message-list"].appendChild(renderMsg(m));
        }
    }

    function captureStreamingNode() {
        const last = el["message-list"].lastElementChild;
        if (!last) return null;
        return {
            bubble: last.querySelector(".bubble"),
            think: last.querySelector(".think-block"),
        };
    }

    function renderMsg(m) {
        const wrap = document.createElement("div");
        wrap.className = "msg " + m.role;
        // user messages keep the role label; assistant replies render the
        // bubbles directly (the think block / bubble speak for themselves)
        if (m.role === "user") {
            const role = document.createElement("div");
            role.className = "msg-role";
            role.textContent = I18N.t("chat.you");
            wrap.appendChild(role);
        }

        if (m.role === "user") {
            if (m.imageNames && m.imageNames.length) {
                const imgs = document.createElement("div");
                m.imageNames.forEach((n) => {
                    const tag = document.createElement("span");
                    tag.className = "attached-chip";
                    tag.textContent = I18N.t("chat.imageAttached") + " " + n;
                    imgs.appendChild(tag);
                });
                wrap.appendChild(imgs);
            }
            if (m.fileNames && m.fileNames.length) {
                const files = document.createElement("div");
                m.fileNames.forEach((n) => {
                    const tag = document.createElement("span");
                    tag.className = "attached-chip";
                    tag.textContent = I18N.t("chat.fileAttached") + " " + n;
                    files.appendChild(tag);
                });
                wrap.appendChild(files);
            }
            const bubble = document.createElement("div");
            bubble.className = "bubble";
            bubble.textContent = m.display || m.content;
            wrap.appendChild(bubble);
        } else {
            const parts = Classification.splitForDisplay(m.content, modelInfo && modelInfo.arch);
            if (parts.think) {
                const tb = document.createElement("div");
                tb.className = "think-block";
                const lbl = document.createElement("span");
                lbl.className = "think-label";
                lbl.textContent = I18N.t("chat.thinkingBlock");
                tb.appendChild(lbl);
                tb.appendChild(document.createTextNode(parts.think));
                wrap.appendChild(tb);
            }
            const bubble = document.createElement("div");
            bubble.className = "bubble";
            bubble.textContent = parts.body || (m.streaming ? "…" : "");
            wrap.appendChild(bubble);
        }
        return wrap;
    }

    function updateEmpty() {
        const empty = messages.length === 0;
        el["chat-empty"].style.display = empty ? "block" : "none";
    }

    function autoGrow() {
        el["chat-input"].style.height = "auto";
        const h = Math.min(el["chat-input"].scrollHeight, 160);
        el["chat-input"].style.height = h + "px";
        // only scroll once the cap is reached
        el["chat-input"].style.overflowY =
            el["chat-input"].scrollHeight > 160 ? "auto" : "hidden";
    }
    function scrollBottom() { el["chat-scroll"].scrollTop = el["chat-scroll"].scrollHeight; }

    /* ---------------- copy conversation to clipboard ---------------- */
    async function exportHistory() {
        if (!messages.length) return;
        const lines = [];
        for (const m of messages) {
            if (m.role === "user") {
                lines.push(`【${I18N.t("chat.you")}】${m.display || m.content}`);
            } else {
                const parts = Classification.split(m.content, modelInfo && modelInfo.arch);
                if (parts.think) {
                    lines.push(`【${I18N.t("chat.thinkingBlock")}】${parts.think}`);
                }
                lines.push(`【${I18N.t("chat.assistant")}】${parts.body}`);
            }
        }
        const text = lines.join("\n\n");
        try {
            await navigator.clipboard.writeText(text);
            showToast(I18N.t("chat.copied"));
        } catch (err) {
            showToast(String(err), true);
        }
    }

    /* ---------------- helpers ---------------- */
    function fmtSize(bytes) {
        if (!bytes) return "0 B";
        const units = ["B", "KB", "MB", "GB", "TB"];
        let i = 0;
        let v = bytes;
        while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
        return v.toFixed(v >= 100 || i === 0 ? 0 : 1) + " " + units[i];
    }
    function fmtTokens(n) {
        if (!n) return "-";
        return n >= 1024 ? Math.round(n / 1024) + "K" : String(n);
    }
})();
