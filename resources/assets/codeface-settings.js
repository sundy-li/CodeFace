((cssText, initialData) => {
  const KEY = "__CODEFACE_SETTINGS__";
  const SYSTEM_ID = "__codeface-system-theme__";
  const UI_VERSION = 9;
  if (window[KEY]?.version === UI_VERSION && window[KEY]?.update) {
    window[KEY].update(cssText, initialData);
    return;
  }
  window[KEY]?.cleanup?.();

  const state = {
    data: initialData,
    commands: [],
    pending: new Map(),
    sequence: 0,
    nativeContent: null,
    entry: null,
    page: null,
    view: "local",
    selectedId: initialData.appliedId || SYSTEM_ID,
    localPreviews: new Map(),
    localPreviewLoading: new Set(),
    market: [],
    marketPreview: null,
    selectedMarketId: null,
    busy: false,
    open: false,
    status: "",
    statusError: false,
  };
  const text = (zh, en) => {
    const appearanceLabel = document.querySelector('[data-settings-panel-slug="appearance"]')?.textContent || "";
    const locale = document.documentElement.lang || navigator.language || "en";
    return /外观/.test(appearanceLabel) || locale.toLowerCase().startsWith("zh") ? zh : en;
  };
  state.status = text("就绪", "Ready");
  const escapeHtml = value => String(value ?? "").replace(/[&<>"]/g, character => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"})[character]);
  const icons = {
    refresh: '<path d="M20 11a8.1 8.1 0 0 0-15.5-2M4 4v5h5"/><path d="M4 13a8.1 8.1 0 0 0 15.5 2M20 20v-5h-5"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    upload: '<path d="M12 16V4m0 0L7 9m5-5 5 5"/><path d="M5 20h14"/>',
    folder: '<path d="M3 6h6l2 2h10v10H3z"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>',
    apply: '<path d="m5 12 4 4L19 6"/>',
    edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4z"/>',
    copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
    download: '<path d="M12 3v12m0 0 5-5m-5 5-5-5"/><path d="M5 21h14"/>',
    trash: '<path d="M3 6h18M8 6V4h8v2m-9 0 1 15h8l1-15"/>',
    update: '<path d="M20 7h-5V2"/><path d="M20 2a9 9 0 1 0 1 10"/>',
    rollback: '<path d="M9 7H4v-5"/><path d="M4 7a9 9 0 1 1-1 9"/>',
    preview: '<path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2.5"/>',
    install: '<path d="M12 3v12m0 0 5-5m-5 5-5-5"/><path d="M4 21h16"/>',
    save: '<path d="M5 3h12l2 2v16H5z"/><path d="M8 3v6h8V3M8 21v-7h8v7"/>',
    close: '<path d="M6 6l12 12M18 6 6 18"/>',
    restart: '<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/>',
    power: '<path d="M12 2v10"/><path d="M18.4 6.6a8 8 0 1 1-12.8 0"/>',
    more: '<circle cx="5" cy="12" r="1" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1" fill="currentColor" stroke="none"/><circle cx="19" cy="12" r="1" fill="currentColor" stroke="none"/>',
  };
  const iconSvg = name => `<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">${icons[name] || icons.apply}</svg>`;
  const iconize = (button, name, label) => {
    if (!button) return;
    button.classList.add("codeface-icon-button");
    button.innerHTML = iconSvg(name);
    button.setAttribute("aria-label", label);
    button.title = label;
  };
  const makeInteractive = (element, handler) => {
    element.tabIndex = 0;
    element.setAttribute("role", "button");
    element.onclick = handler;
    element.onkeydown = event => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      handler();
    };
  };
  const emptyState = message => {
    const node = document.createElement("div");
    node.className = "codeface-empty-state";
    node.innerHTML = `${iconSvg("search")}<span>${escapeHtml(message)}</span>`;
    return node;
  };
  const closeOverflowMenus = except => {
    state.page?.querySelectorAll(".codeface-overflow-menu").forEach(menu => {
      if (menu === except) return;
      menu.hidden = true;
      menu.parentElement?.querySelector("[aria-haspopup=menu]")?.setAttribute("aria-expanded", "false");
    });
  };
  const appendOverflowMenu = (actions, items) => {
    if (!items.length) return;
    const wrapper = document.createElement("div");
    wrapper.className = "codeface-overflow";
    const trigger = document.createElement("button");
    trigger.type = "button";
    trigger.setAttribute("aria-haspopup", "menu");
    trigger.setAttribute("aria-expanded", "false");
    iconize(trigger, "more", text("更多操作", "More actions"));
    const menu = document.createElement("div");
    menu.className = "codeface-overflow-menu";
    menu.setAttribute("role", "menu");
    menu.hidden = true;
    for (const item of items) {
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("role", "menuitem");
      if (item.danger) button.dataset.menuDanger = "true";
      button.innerHTML = `${iconSvg(item.icon)}<span>${escapeHtml(item.label)}</span>`;
      button.onclick = event => {
        event.stopPropagation();
        menu.hidden = true;
        trigger.setAttribute("aria-expanded", "false");
        item.handler();
      };
      menu.appendChild(button);
    }
    trigger.onclick = event => {
      event.stopPropagation();
      const opening = menu.hidden;
      closeOverflowMenus(menu);
      menu.hidden = !opening;
      trigger.setAttribute("aria-expanded", String(opening));
      if (opening) menu.querySelector("button")?.focus();
    };
    wrapper.append(trigger, menu);
    actions.appendChild(wrapper);
  };

  let style = document.getElementById("codeface-settings-style");
  if (!style) {
    style = document.createElement("style");
    style.id = "codeface-settings-style";
    document.head.appendChild(style);
  }
  style.textContent = cssText;

  const setStatus = (message, error = false) => {
    state.status = message;
    state.statusError = error;
    const node = state.page?.querySelector("[data-codeface-status]");
    if (!node) return;
    node.textContent = message;
    node.dataset.error = String(error);
  };

  const request = (type, payload = {}) => new Promise((resolve, reject) => {
    if (state.commands.length >= 8) {
      reject(new Error(text("操作队列已满，请稍后重试", "The operation queue is full; try again shortly")));
      return;
    }
    const requestId = ++state.sequence;
    state.pending.set(requestId, { resolve, reject });
    state.commands.push({ requestId, type, ...payload });
  });

  const run = async (label, type, payload = {}) => {
    if (state.busy) return null;
    state.busy = true;
    state.page?.setAttribute("data-busy", "true");
    setStatus(`${label}…`);
    try {
      const value = await request(type, payload);
      setStatus(text("操作成功", "Operation completed"));
      return value;
    } catch (error) {
      setStatus(error?.message || String(error), true);
      return null;
    } finally {
      state.busy = false;
      state.page?.setAttribute("data-busy", "false");
    }
  };

  const fileBase64 = async (file, maxBytes, label) => {
    if (!file) return "";
    if (file.size > maxBytes) throw new Error(`${label} ${text("文件过大", "is too large")}`);
    const bytes = new Uint8Array(await file.arrayBuffer());
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 0x8000)
      binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
    return btoa(binary);
  };
  const copyText = async value => {
    try { await navigator.clipboard.writeText(value); return; } catch {}
    const textarea = document.createElement("textarea");
    textarea.value = value; textarea.style.position = "fixed"; textarea.style.opacity = "0";
    document.body.appendChild(textarea); textarea.select();
    if (!document.execCommand("copy")) throw new Error(text("复制失败", "Copy failed"));
    textarea.remove();
  };

  const adoptCatalog = value => {
    const catalog = value?.catalog || value;
    if (!catalog?.themes) return;
    const localeChanged = catalog.locale !== state.data.locale;
    state.data = catalog;
    if (localeChanged && !state.statusError)
      state.status = catalog.locale === "zh-CN" ? "操作成功" : "Operation completed";
    if (value?.selectedId) state.selectedId = value.selectedId;
    if (localeChanged && state.page) {
      state.page.remove();
      state.page = null;
      ensure();
    }
    render();
  };

  const themeVisual = (theme, compact = false) => {
    const visual = document.createElement("div");
    visual.className = "codeface-theme-visual";
    visual.style.setProperty("--c1", theme.colors?.[0] || "#fff");
    visual.style.setProperty("--c2", theme.colors?.[1] || "#eee");
    visual.style.setProperty("--c3", theme.colors?.[2] || "#ccc");
    const imageSource = compact ? (theme.thumbnail || theme.preview) : (theme.preview || theme.thumbnail);
    if (imageSource) {
      const image = document.createElement("img");
      image.src = imageSource;
      image.alt = "";
      image.onerror = () => {
        image.remove();
        if (!visual.firstChild) visual.innerHTML = "<span>Aa</span>";
      };
      visual.appendChild(image);
    }
    else visual.innerHTML = "<span>Aa</span>";
    return visual;
  };

  const renderLocal = () => {
    const host = state.page.querySelector("[data-codeface-view=local]");
    const query = host.querySelector("input[type=search]").value.trim().toLowerCase();
    const grid = host.querySelector(".codeface-theme-grid");
    const themes = state.data.themes.filter(theme => `${theme.id} ${theme.name} ${theme.description}`.toLowerCase().includes(query));
    host.querySelector("[data-count]").textContent = `${themes.length} ${text("个主题", "themes")}`;
    grid.replaceChildren(...themes.map(theme => {
      const card = document.createElement("article");
      card.className = "codeface-theme-card";
      card.dataset.themeId = theme.id;
      card.dataset.applied = String(theme.id === state.data.appliedId);
      card.dataset.selected = String(theme.id === state.selectedId);
      card.appendChild(themeVisual(theme, true));
      const body = document.createElement("div"); body.className = "codeface-theme-body";
      const title = document.createElement("strong"); title.textContent = theme.name;
      const description = document.createElement("p"); description.textContent = theme.description;
      const meta = document.createElement("div"); meta.className = "codeface-theme-meta";
      meta.innerHTML = `<span>${theme.id === state.data.appliedId ? text("当前已应用", "Applied") : theme.market ? "CodexThemes" : theme.system ? text("原生主题", "Native") : text("本地主题", "Local")}</span><span class="codeface-theme-dots">${(theme.colors || []).map(color => `<i style="background:${escapeHtml(color)}"></i>`).join("")}</span>`;
      const actions = document.createElement("div"); actions.className = "codeface-card-actions";
      const action = (label, icon, handler, danger = false, disabled = false) => { const button=document.createElement("button");button.type="button";button.disabled=disabled;iconize(button,icon,label);if(danger)button.dataset.danger="true";button.onclick=event=>{event.stopPropagation();handler();};actions.appendChild(button);return button; };
      action(theme.id === state.data.appliedId ? text("已应用", "Applied") : text("应用", "Apply"), "apply", async () => {
        const value = await run(text("正在应用主题", "Applying theme"), "apply", { id: theme.id });
        if (value) adoptCatalog(value);
      }, false, theme.id === state.data.appliedId);
      if (!theme.system) {
        action(text("修改", "Edit"), "edit", () => openEditor(theme.id));
        const overflow = [
          {label:text("复制完整提示词", "Copy full prompt"),icon:"copy",handler:async()=>{const value=await run(text("正在生成提示词","Building prompt"),"prompt",{id:theme.id});if(value?.text){try{await copyText(value.text);setStatus(text("完整提示词已复制","Full prompt copied"));}catch(error){setStatus(error.message,true);}}}},
          {label:text("导出", "Export"),icon:"download",handler:async()=>{const value=await run(text("正在导出", "Exporting"), "export", {id:theme.id});if(value?.path)setStatus(`${text("已导出到", "Exported to")}: ${value.path}`);}},
        ];
        if (theme.market) {
          overflow.push(
            {label:text("检查更新", "Check update"),icon:"search",handler:async()=>{const value=await run(text("正在检查更新", "Checking update"), "check-update", {id:theme.id});if(value)setStatus(value.available ? text("有可用更新", "Update available") : text("已是最新版本", "Up to date"));}},
            {label:text("更新", "Update"),icon:"update",handler:async()=>{const value=await run(text("正在更新", "Updating"), "market-install", {source:theme.id,apply:theme.id===state.data.appliedId});if(value)adoptCatalog(value);}},
            {label:text("回滚", "Rollback"),icon:"rollback",handler:async()=>{const value=await run(text("正在回滚", "Rolling back"), "rollback", {id:theme.id});if(value)adoptCatalog(value);}},
          );
        }
        overflow.push({label:text("删除", "Delete"),icon:"trash",danger:true,handler:async()=>{if(!confirm(text(`确定删除“${theme.name}”吗？`,`Delete “${theme.name}”?`)))return;const value=await run(text("正在删除", "Deleting"), "delete", {id:theme.id});if(value)adoptCatalog(value);}});
        appendOverflowMenu(actions, overflow);
      }
      body.append(title, description, meta, actions); card.appendChild(body);
      makeInteractive(card, () => { state.selectedId = theme.id; renderLocal(); });
      return card;
    }));
    if (!themes.length) grid.appendChild(emptyState(text("没有匹配的本地主题", "No matching local themes")));
    const preview = host.querySelector("[data-local-preview]");
    const selected = themes.find(theme => theme.id === state.selectedId) || themes[0];
    preview.replaceChildren();
    preview.hidden = !selected;
    if (!selected) return;
    state.selectedId = selected.id;
    const previewHeader = document.createElement("header");
    const heading = document.createElement("div");
    const title = document.createElement("h2"); title.textContent = selected.name;
    const description = document.createElement("p"); description.textContent = selected.description;
    heading.append(title, description);
    const status = document.createElement("span");
    status.textContent = selected.id === state.data.appliedId ? text("当前已应用", "Currently applied") : text("预览选择，不会改变 Codex", "Preview selection without changing Codex");
    previewHeader.append(heading, status);
    const visual = themeVisual({...selected, preview: state.localPreviews.get(selected.id) || selected.preview});
    visual.classList.add("codeface-theme-preview-visual");
    const footer = document.createElement("footer");
    const dots = document.createElement("span"); dots.className = "codeface-theme-dots";
    dots.innerHTML = (selected.colors || []).map(color => `<i style="background:${escapeHtml(color)}"></i>`).join("");
    const selectedCard = grid.querySelector(`[data-theme-id="${CSS.escape(selected.id)}"]`);
    const actions = selectedCard?.querySelector(".codeface-card-actions");
    footer.append(dots);
    if (actions) footer.append(actions);
    preview.append(previewHeader, visual, footer);
    if (!selected.system && !state.localPreviews.has(selected.id)) void loadLocalPreview(selected);
  };

  const loadMarketPreview = async theme => {
    state.selectedMarketId = theme.id;
    renderMarket();
    if (state.marketPreview?.theme?.id === theme.id) return;
    const value = await run(text("正在加载市场预览", "Loading market preview"), "market-preview", { theme });
    if (value) {
      state.marketPreview = value;
      renderMarket();
    }
  };

  const loadLocalPreview = async theme => {
    if (theme.system || state.localPreviews.has(theme.id) || state.localPreviewLoading.has(theme.id)) return;
    state.localPreviewLoading.add(theme.id);
    try {
      const value = await request("local-preview", { id: theme.id });
      if (value?.image) state.localPreviews.set(theme.id, value.image);
    } catch (error) {
      setStatus(error?.message || String(error), true);
    } finally {
      state.localPreviewLoading.delete(theme.id);
      renderLocal();
    }
  };

  const renderMarket = () => {
    const list = state.page.querySelector("[data-market-results]");
    const detail = state.page.querySelector("[data-market-preview]");
    const selected = state.market.find(theme => theme.id === state.selectedMarketId) || state.market[0];
    if (selected) state.selectedMarketId = selected.id;
    list.replaceChildren(...state.market.map(theme => {
      const card = document.createElement("article");
      card.className = "codeface-market-card";
      card.dataset.selected = String(theme.id === state.selectedMarketId);
      const thumb = document.createElement("div");
      thumb.className = "codeface-market-thumb";
      if (theme.image) {
        const image = document.createElement("img"); image.src = theme.image; image.alt = ""; image.loading = "lazy"; image.onerror=()=>{image.remove();thumb.innerHTML=iconSvg("preview");}; thumb.appendChild(image);
      } else thumb.innerHTML = iconSvg("preview");
      const body = document.createElement("div");
      const title = document.createElement("strong"); title.textContent = theme.name;
      const description = document.createElement("p"); description.textContent = theme.description || text("没有描述", "No description");
      const meta = document.createElement("small"); meta.textContent = `${theme.author || text("未知作者", "Unknown author")} · ${theme.mode || "theme"}`;
      body.append(title, description, meta);
      card.append(thumb, body);
      makeInteractive(card, () => loadMarketPreview(theme));
      return card;
    }));
    if (!state.market.length) list.appendChild(emptyState(text("没有找到市场主题", "No market themes found")));
    detail.replaceChildren();
    if (!selected) {
      detail.hidden = true;
      return;
    }
    detail.hidden = false;
    const header = document.createElement("header");
    const heading = document.createElement("div");
    const title = document.createElement("h2"); title.textContent = selected.name;
    const description = document.createElement("p"); description.textContent = selected.description || text("没有描述", "No description");
    heading.append(title, description);
    const meta = document.createElement("span"); meta.textContent = `${selected.author || text("未知作者", "Unknown author")} · ${selected.mode || "theme"}`;
    header.append(heading, meta);
    const visual = document.createElement("div");
    visual.className = "codeface-market-preview-visual";
    const previewSource = state.marketPreview?.theme?.id === selected.id
      ? state.marketPreview.image
      : selected.image;
    if (previewSource) {
      const image = document.createElement("img");
      image.src = previewSource;
      image.alt = selected.name;
      image.onerror = () => {
        visual.innerHTML = `<div class="codeface-market-preview-empty">${iconSvg("preview")}<span>${text("效果图加载失败", "Preview image failed to load")}</span></div>`;
      };
      visual.appendChild(image);
    } else {
      visual.innerHTML = `<div class="codeface-market-preview-empty">${iconSvg("preview")}<span>${text("暂无效果图", "No preview image available")}</span></div>`;
    }
    const footer = document.createElement("footer");
    const actions = document.createElement("div"); actions.className = "codeface-card-actions";
    if (selected.installable || selected.kind === "theme") {
      for (const [label, apply] of [[text("安装", "Install"), false], [text("安装并应用", "Install & apply"), true]]) {
        const button = document.createElement("button"); button.type = "button";
        iconize(button, apply ? "apply" : "install", label);
        button.onclick = async () => {
          const value = await run(label, "market-install", { source: selected.id, apply });
          if (value) { adoptCatalog(value); state.view = "local"; render(); }
        };
        actions.appendChild(button);
      }
    } else {
      const reference = document.createElement("span"); reference.className = "codeface-reference-only";
      reference.textContent = text("仅供参考", "Reference only"); actions.appendChild(reference);
    }
    footer.append(actions);
    detail.append(header, visual, footer);
  };

  const openEditor = async id => {
    state.view = "editor"; render();
    const editor = state.page.querySelector("[data-codeface-view=editor]");
    editor.querySelector("[name=existingId]").value = id || "";
    editor.querySelector("[name=image]").value = "";
    if (!id) {
      editor.querySelector("[name=manifest]").value = JSON.stringify({id:"my-theme",name:text("我的主题","My Theme"),description:"",colors:{background:"#111111",panel:"#191919",panelAlt:"#242424",accent:"#7c3aed",accentAlt:"#9b87ff",text:"#f5f5f5",muted:"#a0a0a0",line:"#383838"}}, null, 2);
      editor.querySelector("[name=css]").value = "html.codeface {\n  /* Add theme overrides here. */\n}\n";
      return;
    }
    const value = await run(text("正在读取主题源码", "Loading theme source"), "source", { id });
    if (value) {
      editor.querySelector("[name=manifest]").value = value.manifest;
      editor.querySelector("[name=css]").value = value.css;
    }
  };

  const render = () => {
    if (!state.page) return;
    for (const view of state.page.querySelectorAll("[data-codeface-view]")) view.hidden = view.dataset.codefaceView !== state.view;
    for (const tab of state.page.querySelectorAll("[data-codeface-tab]")) tab.dataset.active = String(tab.dataset.codefaceTab === state.view);
    if (state.view === "local") renderLocal();
    if (state.view === "market") renderMarket();
  };

  const bindPage = page => {
    page.addEventListener("click", event => {
      if (!event.target.closest(".codeface-overflow")) closeOverflowMenus();
    });
    page.addEventListener("keydown", event => {
      if (event.key !== "Escape") return;
      const openMenu = page.querySelector(".codeface-overflow-menu:not([hidden])");
      if (!openMenu) return;
      event.preventDefault();
      const trigger = openMenu.parentElement?.querySelector("[aria-haspopup=menu]");
      closeOverflowMenus();
      trigger?.focus();
    });
    iconize(page.querySelector("[data-refresh]"), "refresh", text("刷新", "Refresh"));
    iconize(page.querySelector("[data-new-theme]"), "plus", text("新建主题", "New theme"));
    iconize(page.querySelector("[data-import]"), "upload", text("导入主题包", "Import package"));
    iconize(page.querySelector("[data-import-directory]"), "folder", text("导入主题目录", "Import folder"));
    iconize(page.querySelector("[data-market-search]"), "search", text("搜索", "Search"));
    iconize(page.querySelector("[data-editor-cancel]"), "close", text("取消", "Cancel"));
    iconize(page.querySelector("[data-editor-save=save]"), "save", text("保存", "Save"));
    iconize(page.querySelector("[data-editor-save=apply]"), "apply", text("保存并应用", "Save & apply"));
    page.querySelectorAll("[data-codeface-tab]").forEach(button => button.onclick=()=>{state.view=button.dataset.codefaceTab;render();if(state.view==="market"&&!state.market.length)page.querySelector("[data-market-search]").click();});
    page.querySelector("[data-new-theme]").onclick=()=>openEditor(null);
    page.querySelector("[data-refresh]").onclick=async()=>{const value=await run(text("正在刷新", "Refreshing"),"refresh");if(value)adoptCatalog(value);};
    page.querySelector("[data-local-search]").oninput=renderLocal;
    page.querySelector("[data-market-search]").onclick=async()=>{const query=page.querySelector("[data-market-query]").value;const value=await run(text("正在搜索市场", "Searching market"),"market-search",{query});if(value){state.market=value;state.marketPreview=null;state.selectedMarketId=value[0]?.id||null;renderMarket();if(value[0])await loadMarketPreview(value[0]);setStatus(`${value.length} ${text("个市场结果","market results")}`);}};
    page.querySelector("[data-market-query]").onkeydown=event=>{if(event.key==="Enter")page.querySelector("[data-market-search]").click();};
    const file=page.querySelector("[data-import-file]");page.querySelector("[data-import]").onclick=()=>file.click();file.onchange=async()=>{const selected=file.files?.[0];if(!selected)return;if(selected.size>30*1024*1024){setStatus(text("主题包不能超过 30 MiB","Theme package cannot exceed 30 MiB"),true);return;}const bytes=new Uint8Array(await selected.arrayBuffer());let binary="";for(let offset=0;offset<bytes.length;offset+=0x8000)binary+=String.fromCharCode(...bytes.subarray(offset,offset+0x8000));const value=await run(text("正在导入主题包","Importing theme package"),"import-package",{base64:btoa(binary)});if(value)adoptCatalog(value);file.value="";};
    const directory=page.querySelector("[data-import-directory-file]");page.querySelector("[data-import-directory]").onclick=()=>directory.click();directory.onchange=async()=>{const selected=[...(directory.files||[])].filter(file=>file.webkitRelativePath.split("/").length===2);try{let total=0;const files=[];for(const item of selected){total+=item.size;if(total>30*1024*1024)throw new Error(text("主题目录不能超过 30 MiB","Theme directory cannot exceed 30 MiB"));files.push({name:item.name,base64:await fileBase64(item,16*1024*1024,item.name)});}const value=await run(text("正在导入主题目录","Importing theme directory"),"import-directory",{files});if(value)adoptCatalog(value);}catch(error){setStatus(error.message,true);}directory.value="";};
    page.querySelector("[data-editor-cancel]").onclick=()=>{state.view="local";render();};
    for(const button of page.querySelectorAll("[data-editor-save]"))button.onclick=async()=>{const editor=page.querySelector("[data-codeface-view=editor]");let imageBase64="";try{imageBase64=await fileBase64(editor.querySelector("[name=image]").files?.[0],16*1024*1024,text("背景图","Background image"));}catch(error){setStatus(error.message,true);return;}const value=await run(button.dataset.editorSave==="apply"?text("正在保存并应用","Saving and applying"):text("正在保存","Saving"),"save",{existingId:editor.querySelector("[name=existingId]").value||null,manifest:editor.querySelector("[name=manifest]").value,css:editor.querySelector("[name=css]").value,imageBase64,apply:button.dataset.editorSave==="apply"});if(value){adoptCatalog(value);state.view="local";render();}};
  };

  const open = () => {
    state.open = true;
    if (!state.page || !state.nativeContent) return;
    state.nativeContent.style.display = "none";
    state.page.dataset.open = "true";
    state.entry?.setAttribute("aria-current", "page");
    render();
  };
  const close = () => {
    state.open = false;
    if (state.nativeContent) state.nativeContent.style.display = "";
    if (state.page) state.page.dataset.open = "false";
    state.entry?.removeAttribute("aria-current");
  };

  const ensure = () => {
    const slug = document.querySelector("[data-settings-panel-slug]");
    if (!slug) { close(); return; }
    const sidebar = slug.closest("div.app-shell-left-panel") || slug.closest("aside") || slug.parentElement?.parentElement;
    const shell = sidebar?.parentElement;
    if (!sidebar || !shell) return;
    if (!state.entry?.isConnected) {
      const nativeEntries = [...document.querySelectorAll("[data-settings-panel-slug]")]
        .map(node => node.closest("button,a") || node);
      const appearanceEntry = nativeEntries.find(node => node.dataset.settingsPanelSlug === "appearance");
      const template = appearanceEntry || nativeEntries[1] || slug.closest("button,a") || slug;
      const entry = template.cloneNode(true);
      entry.removeAttribute("data-settings-panel-slug");
      entry.dataset.codefaceSettingsEntry = "true";
      entry.removeAttribute("aria-current");
      entry.setAttribute("aria-label", "CodeFace");
      const icon = entry.querySelector("svg");
      if (icon) icon.outerHTML = `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" aria-hidden="true"><path d="M14.7 5.6A6 6 0 1 0 14.7 14.4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/><circle cx="15.2" cy="10" r="1.35" fill="currentColor"/></svg>`;
      const label = [...entry.querySelectorAll("span,div")].find(node => node.children.length === 0 && node.textContent.trim());
      if (label) label.textContent = "CodeFace"; else entry.textContent = "CodeFace";
      entry.addEventListener("click", event => { event.preventDefault(); event.stopPropagation(); open(); }, true);
      if (appearanceEntry) appearanceEntry.after(entry); else template.parentElement.appendChild(entry);
      state.entry = entry;
    }
    if (!state.page?.isConnected) {
      const content = [...shell.children].find(node => node !== sidebar);
      if (!content) return;
      state.nativeContent = content;
      const page = document.createElement("main");
      page.id = "codeface-settings-page";
      page.innerHTML = `<div class="codeface-settings-inner">
        <header class="codeface-settings-header"><div><div class="codeface-settings-eyebrow">CodeFace ${escapeHtml(state.data.version)}</div><h1>${text("CodeFace 外观","CodeFace Appearance")}</h1><p>${text("在 Codex 内管理、编辑和切换 CodeFace 主题。","Manage, edit, and switch CodeFace themes without leaving Codex.")}</p></div><div class="codeface-header-actions"><button data-refresh></button><button data-new-theme></button><button data-import></button><input data-import-file type="file" accept=".codex-theme,application/json" hidden><button data-import-directory></button><input data-import-directory-file type="file" webkitdirectory multiple hidden></div></header>
        <nav class="codeface-settings-tabs"><button data-codeface-tab="local">${text("本地主题","Local")}</button><button data-codeface-tab="market">Market</button></nav>
        <div class="codeface-status" data-codeface-status>${text("就绪","Ready")}</div>
        <section data-codeface-view="local"><div class="codeface-settings-toolbar"><input data-local-search type="search" placeholder="${text("搜索本地主题…","Search local themes…")}"><span data-count></span></div><div class="codeface-library-layout"><div class="codeface-theme-grid"></div><article class="codeface-theme-detail" data-local-preview></article></div></section>
        <section data-codeface-view="market" hidden><div class="codeface-settings-toolbar"><input data-market-query type="search" placeholder="${text("搜索 CodexThemes…","Search CodexThemes…")}"><button data-market-search></button></div><div class="codeface-market-layout"><div class="codeface-market-grid" data-market-results></div><article class="codeface-market-preview" data-market-preview hidden></article></div></section>
        <section data-codeface-view="editor" hidden><input name="existingId" type="hidden"><div class="codeface-editor-header"><h2>${text("主题源码","Theme source")}</h2><div><button data-editor-cancel></button><button data-editor-save="save"></button><button data-editor-save="apply"></button></div></div><label>${text("背景图（可选，PNG/JPEG/WebP）","Background image (optional, PNG/JPEG/WebP)")}<input name="image" type="file" accept="image/png,image/jpeg,image/webp"></label><label>theme.json<textarea name="manifest" spellcheck="false"></textarea></label><label>codeface.css<textarea name="css" spellcheck="false"></textarea></label></section>
      </div>`;
      shell.appendChild(page);
      state.page = page;
      bindPage(page);
      page.dataset.busy = String(state.busy);
      setStatus(state.status, state.statusError);
      if (state.open) open();
    }
  };

  const closeOnNativeNavigation = event => { if (event.target.closest?.("[data-settings-panel-slug]")) close(); };
  document.addEventListener("click", closeOnNativeNavigation, true);
  const observer = new MutationObserver(ensure);
  observer.observe(document.documentElement, { childList: true, subtree: true });
  const timer = setInterval(ensure, 1500);
  ensure();

  window[KEY] = {
    version: UI_VERSION,
    drain: () => state.commands.splice(0),
    resolve: (requestId, result) => {
      const pending = state.pending.get(requestId);
      if (!pending) return;
      state.pending.delete(requestId);
      if (result?.ok) pending.resolve(result.value); else pending.reject(new Error(result?.error || "CodeFace operation failed"));
    },
    setApplied: id => { state.data.appliedId = id; if (state.page) render(); },
    diagnostics: () => ({
      busy: state.busy,
      open: state.open,
      status: state.status,
      pending: [...state.pending.keys()],
      queued: state.commands.length,
      view: state.view,
    }),
    update: (nextCss, nextData) => {
      style.textContent = nextCss;
      state.data = nextData;
      if (!state.selectedId || !nextData.themes.some(theme => theme.id === state.selectedId))
        state.selectedId = nextData.appliedId || SYSTEM_ID;
      ensure();
      render();
    },
    cleanup: () => {
      observer.disconnect(); clearInterval(timer);
      document.removeEventListener("click", closeOnNativeNavigation, true);
      for (const pending of state.pending.values()) pending.reject(new Error("CodeFace settings reloaded"));
      state.entry?.remove(); state.page?.remove();
      if (state.nativeContent) state.nativeContent.style.display = "";
      style?.remove();
      delete window[KEY];
    },
    open,
    close,
  };
})(__CODEFACE_SETTINGS_CSS_JSON__, __CODEFACE_SETTINGS_DATA_JSON__);
