/* ============================================================
 * ETET Web Chat - i18n (zh-CN / zh-TW / en)
 * Usage: I18N.t('key') returns the translated string.
 *       I18N.apply() re-translates all [data-i18n] elements.
 * ============================================================ */

const I18N = (() => {
    const messages = {
        "zh-CN": {
            "site.brand": "ETET",

            "footer.copyright": "© 2026 RheoEcho",

            "chat.title": "聊天",
            "chat.exportHistory": "复制对话",
            "chat.copyChat": "复制对话",
            "chat.clearContext": "清除上下文",
            "chat.copied": "已复制到剪贴板（未保存任何文件）",
            "chat.copied": "已复制到剪贴板（未保存任何文件）",
            "chat.switchModel": "换模型",
            "chat.modelLoading": "加载中…",
            "chat.noModel": "未加载模型",
            "chat.selectModelFirst": "请先选择 GGUF 模型。",
            "chat.inputPlaceholder": "输入消息…",
            "chat.send": "发送",
            "chat.stop": "停止",
            "chat.think": "深度思考",
            "chat.web": "联网",
            "chat.image": "图片",
            "chat.file": "文件",
            "chat.you": "你",
            "chat.assistant": "助手",
            "chat.thinkingBlock": "思考过程",
            "chat.generating": "生成中…",
            "chat.emptyChat": "还没有消息，发送第一条消息开始对话吧。",
            "chat.imageAttached": "[图片]",
            "chat.fileAttached": "[文件]",
            "chat.clearConfirmTitle": "清空当前会话",
            "chat.clearConfirmBody": "将删除当前会话的全部消息，且不可撤销。继续吗？",
            "chat.webNeedByok": "联网搜索需要先配置搜索 API（BYOK）。",
            "chat.webSearching": "正在联网搜索…",
            "chat.webFailed": "联网搜索失败：{err}",
            "chat.webInjected": "已注入 {n} 条搜索结果",
            "chat.engineProgress": "引擎：{stage}",

            "modal.confirmTitle": "确认操作",
            "modal.confirmYes": "确定",
            "modal.confirmNo": "取消",

            "setup.subtitle": "本地 GGUF 聊天 · 数据不出设备 · 关闭即清空",
            "setup.pick": "选择 GGUF 模型",
            "setup.pickHint": "支持任意 GGUF 格式大模型；若同目录存在 mmproj 文件将自动识别视觉能力。",
            "setup.repick": "重新选择",
            "setup.next": "下一步",
            "setup.foot": "ETET Preview · 零持久化 · 退出即清空所有数据",
            "setup.name": "模型",
            "setup.arch": "架构",
            "setup.layers": "层数",
            "setup.ctxTrain": "训练上下文",
            "setup.size": "文件大小",
            "setup.mmproj": "视觉 (mmproj)",
            "setup.mmprojYes": "已识别，可发送图片",
            "setup.mmprojNone": "无（图片按钮将不可用）",
            "setup.pickFail": "读取模型失败：{err}",

            "config.title": "运行配置",
            "config.ctx": "上下文长度",
            "config.ctxMax": "模型最大上下文 {k}K",
            "config.ngl": "GPU offload 层数",
            "config.fa": "Flash Attention",
            "config.faOn": "开启",
            "config.unifiedNote": "统一内存设备无需调整显存配比，模型默认全量使用 GPU。",
            "config.gpuMem": "显存占用（预估）",
            "config.ramMem": "内存占用（预估）",
            "config.unifiedMem": "统一内存占用（预估）",
            "config.offloadHint": "拖动滑条调整模型层数在 GPU 与内存间的分配",
            "config.availRam": "可用内存",
            "config.gpu": "GPU 后端",
            "config.start": "开始聊天",
            "config.loadFail": "模型加载失败：{err}",
            "config.loadedOk": "加载完成",
            "config.loadingFile": "正在加载模型（{size}）",
            "config.elapsed": "已用 {s} 秒",

            "byok.title": "联网搜索 API（BYOK）",
            "byok.note": "自带 Key，仅保存在本次会话内存中，退出应用即清空。不配置也可完全离线使用。",
            "byok.provider": "服务商",
            "byok.custom": "自定义",
            "byok.url": "请求 URL",
            "byok.key": "API Key",
            "byok.skip": "暂不配置",
            "byok.save": "保存",
            "byok.saved": "已保存（仅本次会话）",
            "byok.cleared": "已清除，本次会话将离线运行"
        },
        "zh-TW": {
            "site.brand": "ETET",

            "footer.copyright": "© 2026 RheoEcho",

            "chat.title": "聊天",
            "chat.exportHistory": "複製對話",
            "chat.copyChat": "複製對話",
            "chat.clearContext": "清除上下文",
            "chat.copied": "已複製到剪貼簿（未儲存任何檔案）",
            "chat.copied": "已複製到剪貼簿（未儲存任何檔案）",
            "chat.switchModel": "換模型",
            "chat.modelLoading": "載入中…",
            "chat.noModel": "未載入模型",
            "chat.selectModelFirst": "請先選擇 GGUF 模型。",
            "chat.inputPlaceholder": "輸入訊息…",
            "chat.send": "傳送",
            "chat.stop": "停止",
            "chat.think": "深度思考",
            "chat.web": "聯網",
            "chat.image": "圖片",
            "chat.file": "檔案",
            "chat.you": "你",
            "chat.assistant": "助手",
            "chat.thinkingBlock": "思考過程",
            "chat.generating": "生成中…",
            "chat.emptyChat": "還沒有訊息，傳送第一條訊息開始對話吧。",
            "chat.imageAttached": "[圖片]",
            "chat.fileAttached": "[檔案]",
            "chat.clearConfirmTitle": "清空目前會話",
            "chat.clearConfirmBody": "將刪除目前會話的全部訊息，且不可復原。繼續嗎？",
            "chat.webNeedByok": "聯網搜尋需要先設定搜尋 API（BYOK）。",
            "chat.webSearching": "正在聯網搜尋…",
            "chat.webFailed": "聯網搜尋失敗：{err}",
            "chat.webInjected": "已注入 {n} 筆搜尋結果",
            "chat.engineProgress": "引擎：{stage}",

            "modal.confirmTitle": "確認操作",
            "modal.confirmYes": "確定",
            "modal.confirmNo": "取消",

            "setup.subtitle": "本地 GGUF 聊天 · 資料不出裝置 · 關閉即清空",
            "setup.pick": "選擇 GGUF 模型",
            "setup.pickHint": "支援任意 GGUF 格式大模型；若同目錄存在 mmproj 檔案將自動識別視覺能力。",
            "setup.repick": "重新選擇",
            "setup.next": "下一步",
            "setup.foot": "ETET Preview · 零持久化 · 退出即清空所有資料",
            "setup.name": "模型",
            "setup.arch": "架構",
            "setup.layers": "層數",
            "setup.ctxTrain": "訓練上下文",
            "setup.size": "檔案大小",
            "setup.mmproj": "視覺 (mmproj)",
            "setup.mmprojYes": "已識別，可傳送圖片",
            "setup.mmprojNone": "無（圖片按鈕將不可用）",
            "setup.pickFail": "讀取模型失敗：{err}",

            "config.title": "執行設定",
            "config.ctx": "上下文長度",
            "config.ctxMax": "模型最大上下文 {k}K",
            "config.ngl": "GPU offload 層數",
            "config.fa": "Flash Attention",
            "config.faOn": "開啟",
            "config.unifiedNote": "統一記憶體裝置無需調整顯示記憶體配比，模型預設全量使用 GPU。",
            "config.gpuMem": "顯示記憶體佔用（預估）",
            "config.ramMem": "記憶體佔用（預估）",
            "config.unifiedMem": "統一記憶體佔用（預估）",
            "config.offloadHint": "拖動滑條調整模型層數在 GPU 與記憶體間的分配",
            "config.availRam": "可用記憶體",
            "config.gpu": "GPU 後端",
            "config.start": "開始聊天",
            "config.loadFail": "模型載入失敗：{err}",
            "config.loadedOk": "載入完成",
            "config.loadingFile": "正在載入模型（{size}）",
            "config.elapsed": "已用 {s} 秒",

            "byok.title": "聯網搜尋 API（BYOK）",
            "byok.note": "自帶 Key，僅保存在本次會話記憶體中，退出應用即清空。不設定也可完全離線使用。",
            "byok.provider": "服務商",
            "byok.custom": "自訂",
            "byok.url": "請求 URL",
            "byok.key": "API Key",
            "byok.skip": "暫不設定",
            "byok.save": "儲存",
            "byok.saved": "已儲存（僅本次會話）",
            "byok.cleared": "已清除，本次會話將離線執行"
        },
        "en": {
            "site.brand": "ETET",

            "footer.copyright": "© 2026 RheoEcho",

            "chat.title": "Chat",
            "chat.exportHistory": "Copy Chat",
            "chat.copyChat": "Copy Chat",
            "chat.clearContext": "Clear Context",
            "chat.copied": "Copied to clipboard (no file saved)",
            "chat.copied": "Copied to clipboard (no file saved)",
            "chat.switchModel": "Model",
            "chat.modelLoading": "Loading…",
            "chat.noModel": "No model loaded",
            "chat.selectModelFirst": "Please select a GGUF model first.",
            "chat.inputPlaceholder": "Type a message…",
            "chat.send": "Send",
            "chat.stop": "Stop",
            "chat.think": "Deep Think",
            "chat.web": "Web",
            "chat.image": "Image",
            "chat.file": "File",
            "chat.you": "You",
            "chat.assistant": "Assistant",
            "chat.thinkingBlock": "Thinking",
            "chat.generating": "Generating…",
            "chat.emptyChat": "No messages yet. Send the first message to start the conversation.",
            "chat.imageAttached": "[Image]",
            "chat.fileAttached": "[File]",
            "chat.clearConfirmTitle": "Clear current session",
            "chat.clearConfirmBody": "All messages in this session will be deleted. This cannot be undone. Continue?",
            "chat.webNeedByok": "Web search needs a search API key (BYOK) first.",
            "chat.webSearching": "Searching the web…",
            "chat.webFailed": "Web search failed: {err}",
            "chat.webInjected": "Injected {n} search results",
            "chat.engineProgress": "Engine: {stage}",

            "modal.confirmTitle": "Confirm",
            "modal.confirmYes": "OK",
            "modal.confirmNo": "Cancel",

            "setup.subtitle": "Local GGUF chat · Data never leaves your device · Cleared on exit",
            "setup.pick": "Select GGUF Model",
            "setup.pickHint": "Any GGUF model is supported. If an mmproj file exists in the same directory, vision is detected automatically.",
            "setup.repick": "Pick again",
            "setup.next": "Next",
            "setup.foot": "ETET Preview · Zero persistence · Everything is cleared on exit",
            "setup.name": "Model",
            "setup.arch": "Architecture",
            "setup.layers": "Layers",
            "setup.ctxTrain": "Train ctx",
            "setup.size": "File size",
            "setup.mmproj": "Vision (mmproj)",
            "setup.mmprojYes": "Detected, image input enabled",
            "setup.mmprojNone": "None (image button will be disabled)",
            "setup.pickFail": "Failed to read model: {err}",

            "config.title": "Run Configuration",
            "config.ctx": "Context length",
            "config.ctxMax": "Model max context: {k}K",
            "config.ngl": "GPU offload layers",
            "config.fa": "Flash Attention",
            "config.faOn": "On",
            "config.unifiedNote": "Unified-memory devices don't need memory/VRAM split; the model uses the GPU fully by default.",
            "config.gpuMem": "VRAM usage (est.)",
            "config.ramMem": "RAM usage (est.)",
            "config.unifiedMem": "Unified memory usage (est.)",
            "config.offloadHint": "Drag to split model layers between GPU and RAM",
            "config.availRam": "Available RAM",
            "config.gpu": "GPU backend",
            "config.start": "Start Chat",
            "config.loadFail": "Model load failed: {err}",
            "config.loadedOk": "Load complete",
            "config.loadingFile": "Loading model ({size})",
            "config.elapsed": "{s}s elapsed",

            "byok.title": "Web Search API (BYOK)",
            "byok.note": "Bring your own key. Kept in session memory only and cleared when the app exits. Fully offline use works without any key.",
            "byok.provider": "Provider",
            "byok.custom": "Custom",
            "byok.url": "Request URL",
            "byok.key": "API Key",
            "byok.skip": "Not now",
            "byok.save": "Save",
            "byok.saved": "Saved (this session only)",
            "byok.cleared": "Cleared. This session will run offline."
        }
    };

    // Zero-persistence: language is session-only. Defaults to the system
    // language; the user can switch on the model screen (not persisted).
    function detectLang() {
        const nav = (navigator.language || "zh-CN").toLowerCase();
        if (nav.startsWith("zh")) {
            return /tw|hk|mo|hant/.test(nav) ? "zh-TW" : "zh-CN";
        }
        if (nav.startsWith("en")) return "en";
        return "zh-CN";
    }

    let current = detectLang();

    function t(key, params) {
        let str = (messages[current] && messages[current][key]) ||
                  messages["en"][key] || key;
        if (params) {
            for (const k of Object.keys(params)) {
                str = str.split("{" + k + "}").join(params[k]);
            }
        }
        return str;
    }

    /** Re-translate every element carrying data-i18n / data-i18n-placeholder */
    function apply() {
        document.querySelectorAll("[data-i18n]").forEach(el => {
            el.textContent = t(el.getAttribute("data-i18n"));
        });
        document.querySelectorAll("[data-i18n-placeholder]").forEach(el => {
            el.setAttribute("placeholder", t(el.getAttribute("data-i18n-placeholder")));
        });
        document.documentElement.lang = current;
    }

    function setLang(lang) {
        if (!messages[lang]) return;
        current = lang;
        apply();
        document.dispatchEvent(new CustomEvent("etetlangchange", { detail: lang }));
    }

    function getLang() {
        return current;
    }

    return { t, apply, setLang, getLang };
})();
