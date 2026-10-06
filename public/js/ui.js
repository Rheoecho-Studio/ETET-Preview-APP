/* ============================================================
 * ETET Web Chat - Shared UI helpers
 * Navbar injection, language switcher, modal, toast.
 * ============================================================ */

/* ---------- Navbar (injected into #navbar placeholder) ---------- */
function buildNavbar(activePage) {
    const host = document.getElementById("navbar");
    if (!host) return;
    const links = [
        { id: "index", href: "index.html", key: "nav.index" },
        { id: "chat", href: "chat.html", key: "nav.chat" },
        { id: "historyeditor", href: "historyeditor.html", key: "nav.historyeditor" }
    ];
    const linkHtml = links.map(l =>
        `<a href="${l.href}" class="${l.id === activePage ? "active" : ""}" data-i18n="${l.key}"></a>`
    ).join("");
    host.innerHTML = `
        <a class="nav-brand" href="index.html">
            <img src="etetlogo.png" alt="ETET logo">
            <span data-i18n="site.brand"></span>
        </a>
        <div class="nav-links">
            ${linkHtml}
            <div class="lang-switch">
                <button type="button" data-lang="zh-CN">简</button>
                <button type="button" data-lang="zh-TW">繁</button>
                <button type="button" data-lang="en">EN</button>
            </div>
        </div>`;
    host.querySelectorAll(".lang-switch button").forEach(btn => {
        btn.addEventListener("click", () => I18N.setLang(btn.dataset.lang));
    });
    markLangButtons();
}

function markLangButtons() {
    document.querySelectorAll(".lang-switch button").forEach(btn => {
        btn.classList.toggle("active", btn.dataset.lang === I18N.getLang());
    });
}

document.addEventListener("etetlangchange", markLangButtons);

/* ---------- Modal confirm dialog ---------- */
/**
 * Show a confirm modal. Returns a Promise<boolean>.
 */
function showConfirm(titleKey, bodyKey, bodyParams) {
    return new Promise(resolve => {
        let modal = document.getElementById("etet-confirm-modal");
        if (!modal) {
            modal = document.createElement("div");
            modal.id = "etet-confirm-modal";
            modal.className = "popup-modal";
            modal.innerHTML = `
                <div class="popup-content">
                    <span class="popup-close" id="etet-confirm-x">&times;</span>
                    <h3 class="popup-title" id="etet-confirm-title"></h3>
                    <p class="card-text" id="etet-confirm-body" style="margin-bottom:1.5rem;"></p>
                    <div style="display:flex;gap:0.8rem;justify-content:flex-end;">
                        <button class="ghost-button" id="etet-confirm-no"></button>
                        <button class="pixel-button" id="etet-confirm-yes" style="padding:0.6rem 1.8rem;"></button>
                    </div>
                </div>`;
            document.body.appendChild(modal);
        }
        const titleEl = modal.querySelector("#etet-confirm-title");
        const bodyEl = modal.querySelector("#etet-confirm-body");
        const yesBtn = modal.querySelector("#etet-confirm-yes");
        const noBtn = modal.querySelector("#etet-confirm-no");
        const xBtn = modal.querySelector("#etet-confirm-x");

        titleEl.textContent = I18N.t(titleKey);
        bodyEl.textContent = I18N.t(bodyKey, bodyParams);
        yesBtn.textContent = I18N.t("modal.confirmYes");
        noBtn.textContent = I18N.t("modal.confirmNo");

        function close(result) {
            modal.classList.remove("open");
            yesBtn.onclick = noBtn.onclick = xBtn.onclick = null;
            resolve(result);
        }
        yesBtn.onclick = () => close(true);
        noBtn.onclick = () => close(false);
        xBtn.onclick = () => close(false);
        modal.classList.add("open");
    });
}

/* ---------- Toast ---------- */
function showToast(message, isError) {
    let container = document.querySelector(".toast-container");
    if (!container) {
        container = document.createElement("div");
        container.className = "toast-container";
        document.body.appendChild(container);
    }
    const toast = document.createElement("div");
    toast.className = "toast" + (isError ? " toast-error" : "");
    toast.textContent = message;
    container.appendChild(toast);
    setTimeout(() => toast.remove(), 4000);
}

/* ---------- Footer (injected into #footer placeholder) ---------- */
function buildFooter() {
    const host = document.getElementById("footer");
    if (!host) return;
    host.className = "footer";
    host.innerHTML = `
        <div class="footer-content">
            <div class="footer-logo">
                <img src="logo.png" alt="ETET logo">
                <div class="footer-text">
                    <span class="footer-brand">RheoEcho ETET</span>
                    <span class="footer-copyright" data-i18n="footer.copyright"></span>
                </div>
            </div>
        </div>`;
}

/* ---------- Common init on DOM ready ---------- */
document.addEventListener("DOMContentLoaded", () => {
    I18N.apply();
});
