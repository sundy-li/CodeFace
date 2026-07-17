((cssText, artDataUrl, themeConfig) => {
  const STATE_KEY = "__CODEFACE_STATE__";
  const DISABLED_KEY = "__CODEFACE_DISABLED__";
  const STYLE_ID = "codeface-style";
  const CHROME_ID = "codeface-chrome";
  const SHELL_ATTR = "data-codeface-shell";
  const VERSION = __CODEFACE_VERSION_JSON__;
  const THEME = themeConfig && typeof themeConfig === "object" ? themeConfig : {};
  const THEME_VARIABLES = [
    "--cf-bg", "--cf-panel", "--cf-panel-2", "--cf-green", "--cf-lime",
    "--cf-cyan", "--cf-purple", "--cf-text", "--cf-muted", "--cf-line",
    "--codeface-name", "--codeface-tagline", "--codeface-project-prefix",
    "--codeface-project-label", "--codeface-hero-art", "--codeface-hero-fit", "--codeface-hero-position",
    "--codeface-hero-foreground-opacity", "--codeface-hero-backdrop-opacity",
    "--codeface-hero-panel-opacity",
    "--codeface-background-position",
    "--codeface-task-overlay", "--codeface-task-overlay-soft",
    "--codeface-suggestion-1-title", "--codeface-suggestion-1-description",
    "--codeface-suggestion-2-title", "--codeface-suggestion-2-description",
    "--codeface-suggestion-3-title", "--codeface-suggestion-3-description",
    "--codeface-suggestion-4-title", "--codeface-suggestion-4-description",
  ];
  window[DISABLED_KEY] = false;

  const previous = window[STATE_KEY];
  if (previous?.observer) previous.observer.disconnect();
  if (previous?.timer) clearInterval(previous.timer);
  if (previous?.scheduler?.timeout) clearTimeout(previous.scheduler.timeout);
  if (previous?.resizeHandler) window.removeEventListener("resize", previous.resizeHandler);
  if (previous?.mediaHandler && previous?.mediaQuery) {
    try { previous.mediaQuery.removeEventListener("change", previous.mediaHandler); } catch {}
  }
  if (previous?.artUrl) URL.revokeObjectURL(previous.artUrl);

  const artUrl = (() => {
    const comma = artDataUrl.indexOf(",");
    const mime = /^data:([^;,]+)/.exec(artDataUrl)?.[1] || "image/png";
    const binary = atob(artDataUrl.slice(comma + 1));
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return URL.createObjectURL(new Blob([bytes], { type: mime }));
  })();

  const cssString = (value) => JSON.stringify(String(value ?? ""));

  const parseRgb = (value) => {
    if (!value || value === "transparent") return null;
    const m = String(value).match(/rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i);
    if (!m) return null;
    return { r: Number(m[1]), g: Number(m[2]), b: Number(m[3]) };
  };

  const luminance = ({ r, g, b }) => {
    const lin = [r, g, b].map((c) => {
      const x = c / 255;
      return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
  };

  /** Detect Codex app light/dark shell for CSS branching. */
  const detectShellMode = () => {
    const root = document.documentElement;
    const body = document.body;
    const cls = `${root.className || ""} ${body?.className || ""}`.toLowerCase();

    if (/\b(dark|theme-dark|appearance-dark)\b/.test(cls)) return "dark";
    if (/\b(light|theme-light|appearance-light)\b/.test(cls)) return "light";

    const dataTheme = (
      root.getAttribute("data-theme") ||
      root.getAttribute("data-appearance") ||
      root.getAttribute("data-color-mode") ||
      body?.getAttribute("data-theme") ||
      body?.getAttribute("data-appearance") ||
      ""
    ).toLowerCase();
    if (dataTheme.includes("dark")) return "dark";
    if (dataTheme.includes("light")) return "light";

    // Radios in profile menu (if present in DOM)
    const checked = document.querySelector('input[name="appearance-theme"]:checked');
    if (checked) {
      const label = (checked.getAttribute("aria-label") || checked.value || "").toLowerCase();
      if (label.includes("暗") || label.includes("dark")) return "dark";
      if (label.includes("浅") || label.includes("light")) return "light";
      if (label.includes("系统") || label.includes("system")) {
        return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
      }
    }

    try {
      const cs = getComputedStyle(root).colorScheme || "";
      if (cs.includes("dark") && !cs.includes("light")) return "dark";
      if (cs.includes("light") && !cs.includes("dark")) return "light";
    } catch {}

    // Background luminance of main surfaces
    const samples = [
      body,
      document.querySelector("main.main-surface"),
      document.querySelector("aside.app-shell-left-panel"),
    ].filter(Boolean);
    let votesLight = 0;
    let votesDark = 0;
    for (const el of samples) {
      try {
        const rgb = parseRgb(getComputedStyle(el).backgroundColor);
        if (!rgb) continue;
        const L = luminance(rgb);
        if (L >= 0.55) votesLight += 1;
        else if (L <= 0.25) votesDark += 1;
      } catch {}
    }
    if (votesLight > votesDark) return "light";
    if (votesDark > votesLight) return "dark";

    try {
      if (window.matchMedia("(prefers-color-scheme: dark)").matches) return "dark";
    } catch {}
    return "light";
  };

  const applyTheme = (root, shell) => {
    const colors = THEME.colors || {};
    const accent = colors.accent || (shell === "light" ? "#e25563" : "#7cff46");
    const accentAlt = colors.accentAlt || accent;
    const secondary = colors.secondary || (shell === "light" ? "#f3a8af" : "#36d7e8");
    const highlight = colors.highlight || (shell === "light" ? "#c93d4c" : "#642a8c");

    let variables;
    if (shell === "light") {
      variables = {
        "--cf-bg": colors.background || "#f6f2f3",
        "--cf-panel": colors.panel || "#ffffff",
        "--cf-panel-2": colors.panelAlt || "#fff7f8",
        "--cf-green": accent,
        "--cf-lime": accentAlt,
        "--cf-cyan": secondary,
        "--cf-purple": highlight,
        "--cf-text": colors.text || "#1f1a1b",
        "--cf-muted": colors.muted || "#6b5f62",
        "--cf-line": colors.line || "rgba(196, 120, 128, .22)",
      };
    } else {
      variables = {
        "--cf-bg": colors.background || "#071116",
        "--cf-panel": colors.panel || "#0b1a20",
        "--cf-panel-2": colors.panelAlt || "#10272c",
        "--cf-green": accent,
        "--cf-lime": accentAlt,
        "--cf-cyan": secondary,
        "--cf-purple": highlight,
        "--cf-text": colors.text || "#e9fff1",
        "--cf-muted": colors.muted || "#9ebdb3",
        "--cf-line": colors.line || "rgba(124, 255, 70, .28)",
      };
    }

    for (const [name, value] of Object.entries(variables)) {
      if (typeof value === "string" && value) root.style.setProperty(name, value);
    }
    root.style.setProperty("--codeface-name", cssString(THEME.name || "CodeFace"));
    root.style.setProperty("--codeface-tagline", cssString(THEME.tagline || "Make something wonderful."));
    root.style.setProperty("--codeface-project-prefix", cssString(THEME.projectPrefix || "选择项目 · "));
    root.style.setProperty("--codeface-project-label", cssString(THEME.projectLabel || "◉  选择项目"));
    const suggestions = Array.isArray(THEME.suggestions) ? THEME.suggestions : [];
    const suggestionDefaults = [
      ["构建", "编写代码与应用"],
      ["分析", "理解代码与数据"],
      ["自动化", "处理重复工作流"],
      ["调试", "定位问题并修复"],
    ];
    suggestionDefaults.forEach(([defaultTitle, defaultDescription], index) => {
      const suggestion = suggestions[index] || {};
      const title = typeof suggestion.title === "string" && suggestion.title.trim()
        ? suggestion.title.trim() : defaultTitle;
      const description = typeof suggestion.description === "string" && suggestion.description.trim()
        ? suggestion.description.trim() : defaultDescription;
      root.style.setProperty(`--codeface-suggestion-${index + 1}-title`, cssString(title));
      root.style.setProperty(`--codeface-suggestion-${index + 1}-description`, cssString(description));
    });
    const heroFit = new Set(["none", "contain", "cover"]).has(THEME.layout?.heroFit)
      ? THEME.layout.heroFit : "cover";
    root.setAttribute("data-codeface-hero-fit", heroFit);
    const heroPositions = new Set(["center center", "right center", "center 20%"]);
    const backgroundPositions = new Set(["center center", "center 20%", "center 35%"]);
    const heroPosition = heroPositions.has(THEME.layout?.heroPosition)
      ? THEME.layout.heroPosition : "center center";
    const backgroundPosition = backgroundPositions.has(THEME.layout?.backgroundPosition)
      ? THEME.layout.backgroundPosition : "center center";
    const taskOverlay = Number.isFinite(THEME.layout?.taskOverlay)
      ? Math.min(.9, Math.max(.35, THEME.layout.taskOverlay)) : .68;
    root.style.setProperty("--codeface-hero-art", heroFit === "none" ? "none" : "var(--codeface-art)");
    root.style.setProperty("--codeface-hero-fit", heroFit);
    root.style.setProperty("--codeface-hero-position", heroPosition);
    root.style.setProperty("--codeface-hero-foreground-opacity", heroFit === "contain" ? "1" : "0");
    root.style.setProperty("--codeface-hero-backdrop-opacity", heroFit === "contain" ? ".58" : "0");
    root.style.setProperty("--codeface-hero-panel-opacity", heroFit === "none" ? ".68" : "1");
    root.style.setProperty("--codeface-background-position", backgroundPosition);
    root.style.setProperty("--codeface-task-overlay", String(taskOverlay));
    root.style.setProperty("--codeface-task-overlay-soft", String(Math.max(.22, taskOverlay - .22)));
  };

  const existingStyle = document.getElementById(STYLE_ID);
  if (existingStyle) {
    existingStyle.textContent = cssText;
    existingStyle.dataset.codefaceVersion = VERSION;
  }

  const ensure = () => {
    if (window[DISABLED_KEY]) return;
    const root = document.documentElement;
    if (!root) return;
    const shell = detectShellMode();
    root.classList.add("codeface");
    root.setAttribute(SHELL_ATTR, shell);
    root.style.setProperty("--codeface-art", `url("${artUrl}")`);
    applyTheme(root, shell);

    let style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      (document.head || root).appendChild(style);
    }
    if (style.dataset.codefaceVersion !== VERSION) {
      style.textContent = cssText;
      style.dataset.codefaceVersion = VERSION;
    }

    const shellMain = document.querySelector("main.main-surface") || document.querySelector("main");
    const homeIndicator = document.querySelector('[data-testid="home-icon"]');
    const home = homeIndicator?.closest('[role="main"]') ||
      [...document.querySelectorAll('[role="main"]')].find((candidate) =>
        candidate.querySelector('[data-feature="game-source"]') &&
        candidate.querySelector('.group\\\\/home-suggestions')) || null;
    for (const candidate of document.querySelectorAll('[role="main"].codeface-home')) {
      if (candidate !== home) candidate.classList.remove("codeface-home");
    }
    if (home) home.classList.add("codeface-home");
    if (home) {
      home.querySelectorAll('.group\\/home-suggestions button').forEach((button, index) => {
        button.dataset.codefaceSuggestion = String(index + 1);
      });
    }

    if (!shellMain || !document.body) return;
    root.classList.toggle("codeface-home-route", Boolean(home));
    shellMain.classList.toggle("codeface-home-shell", Boolean(home));
    let chrome = document.getElementById(CHROME_ID);
    if (!chrome || chrome.parentElement !== document.body) {
      chrome?.remove();
      chrome = document.createElement("div");
      chrome.id = CHROME_ID;
      chrome.setAttribute("aria-hidden", "true");
      chrome.innerHTML = `
        <div class="codeface-brand">
          <span class="codeface-portal-mark">◉</span>
          <span><b></b><small></small></span>
        </div>
        <div class="codeface-status"><i></i><span></span></div>
        <div class="codeface-quote"></div>
        <div class="codeface-particles"><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i></div>
        <div class="codeface-orbit"></div>`;
      document.body.appendChild(chrome);
    }
    chrome.querySelector(".codeface-brand b").textContent = THEME.name || "CodeFace";
    chrome.querySelector(".codeface-brand small").textContent = THEME.brandSubtitle || "CODEFACE THEME";
    chrome.querySelector(".codeface-status span").textContent = THEME.statusText || "CODEFACE ONLINE";
    chrome.querySelector(".codeface-quote").textContent = THEME.quote || "MAKE SOMETHING WONDERFUL";
    const shellBox = shellMain.getBoundingClientRect();
    chrome.style.left = `${Math.round(shellBox.left)}px`;
    chrome.style.top = `${Math.round(shellBox.top)}px`;
    chrome.style.width = `${Math.round(shellBox.width)}px`;
    chrome.style.height = `${Math.round(shellBox.height)}px`;
    chrome.classList.toggle("codeface-home-shell", Boolean(home));
    chrome.dataset.codefaceShell = shell;
  };

  const cleanup = () => {
    window[DISABLED_KEY] = true;
    document.documentElement?.classList.remove("codeface");
    document.documentElement?.classList.remove("codeface-home-route");
    document.documentElement?.removeAttribute(SHELL_ATTR);
    document.documentElement?.removeAttribute("data-codeface-hero-fit");
    document.documentElement?.style.removeProperty("--codeface-art");
    for (const name of THEME_VARIABLES) document.documentElement?.style.removeProperty(name);
    document.querySelectorAll(".codeface-home").forEach((node) => node.classList.remove("codeface-home"));
    document.querySelectorAll(".codeface-home-shell").forEach((node) => node.classList.remove("codeface-home-shell"));
    document.getElementById(STYLE_ID)?.remove();
    document.getElementById(CHROME_ID)?.remove();
    const state = window[STATE_KEY];
    state?.observer?.disconnect();
    if (state?.timer) clearInterval(state.timer);
    if (state?.scheduler?.timeout) clearTimeout(state.scheduler.timeout);
    if (state?.resizeHandler) window.removeEventListener("resize", state.resizeHandler);
    if (state?.mediaHandler && state?.mediaQuery) {
      try { state.mediaQuery.removeEventListener("change", state.mediaHandler); } catch {}
    }
    if (state?.artUrl) URL.revokeObjectURL(state.artUrl);
    delete window[STATE_KEY];
    return true;
  };

  const scheduler = { timeout: null };
  const scheduleEnsure = () => {
    if (scheduler.timeout) clearTimeout(scheduler.timeout);
    scheduler.timeout = setTimeout(() => {
      scheduler.timeout = null;
      ensure();
    }, 180);
  };
  const observer = new MutationObserver(scheduleEnsure);
  observer.observe(document.documentElement, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["class", "data-theme", "data-appearance", "data-color-mode", "style"],
  });
  const timer = setInterval(ensure, 4000);
  const resizeHandler = scheduleEnsure;
  window.addEventListener("resize", resizeHandler, { passive: true });

  let mediaQuery = null;
  let mediaHandler = null;
  try {
    mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    mediaHandler = () => scheduleEnsure();
    mediaQuery.addEventListener("change", mediaHandler);
  } catch {}

  window[STATE_KEY] = {
    ensure,
    cleanup,
    observer,
    timer,
    scheduler,
    resizeHandler,
    mediaQuery,
    mediaHandler,
    artUrl,
    version: VERSION,
    themeId: THEME.id || "custom",
    detectShellMode,
  };
  ensure();
  return { installed: true, version: VERSION, themeId: THEME.id || "custom", shell: detectShellMode() };
})(__CODEFACE_CSS_JSON__, __CODEFACE_ART_JSON__, __CODEFACE_THEME_JSON__)
