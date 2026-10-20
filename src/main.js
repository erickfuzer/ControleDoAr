import { T, label, listText, applyStatic } from "./i18n.js";

// Fora do Tauri (abrindo o HTML num navegador comum), usa um backend falso para desenvolver a UI.
const tauri = window.__TAURI__;
const { invoke, listen } = tauri
  ? { invoke: tauri.core.invoke, listen: tauri.event.listen }
  : await import("./dev-mock.js");

const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];
const app = $("#app");

let snap = null; // último snapshot do backend
let pendingTemp = null; // temperatura escolhida e ainda não enviada
let tempTimer = null;
let view = "main";

const fmt = (t) => (Number.isInteger(t) ? String(t) : t.toFixed(1));
const deg = (t, unit) => `${fmt(t)}°${unit}`;
const has = (list, v) => Array.isArray(list) && list.includes(v);

// ---------- Render ----------

function render() {
  if (!snap) return;
  if (snap.conn === "noToken" && view !== "login") showView("login");

  const s = snap.state;
  const f = snap.features;
  $("#device-label").textContent = snap.deviceLabel || T.airSection;
  $("#place").textContent = snap.place || " ";
  app.classList.toggle("loading", !s || !f);
  renderStatus();
  if (!s || !f || snap.conn === "noToken") {
    $("#filter-chip").hidden = true;
    return;
  }

  const on = s.power && s.online;
  app.dataset.mode = s.mode;
  app.classList.toggle("on", on);
  app.classList.toggle("no-temp", s.mode === "wind");

  // Banner
  if (!s.online) {
    $("#hero-title").textContent = T.offline;
    $("#hero-temp").textContent = "";
  } else if (s.power) {
    $("#hero-title").textContent = T.state[s.mode] || label("modes", s.mode);
    const hero = $("#hero-temp");
    hero.textContent = s.roomTemp != null ? deg(s.roomTemp, f.unit) : deg(s.setpoint, f.unit);
    if (s.humidity != null) {
      const small = document.createElement("small");
      small.textContent = `${Math.round(s.humidity)}%`;
      hero.append(small);
    }
  } else {
    $("#hero-title").textContent = T.off;
    $("#hero-temp").textContent = "";
  }

  // Modo e temperatura
  $("#mode-val").textContent = label("modes", s.mode);
  $("#tile-mode").hidden = f.modes.length === 0;
  $("#temp-note").hidden = s.mode !== "wind";
  const slider = $("#slider");
  slider.min = f.tempMin;
  slider.max = f.tempMax;
  slider.step = f.tempStep;
  $("#t-min").textContent = fmt(f.tempMin);
  $("#t-max").textContent = fmt(f.tempMax);
  setTempDisplay(pendingTemp ?? s.setpoint);

  // Vento
  const hasFan = f.fanModes.length > 0;
  const hasOsc = f.oscillationModes.length > 0;
  $("#tile-fan").hidden = !hasFan;
  $("#tile-osc").hidden = !hasOsc;
  $("#pair").hidden = !hasFan && !hasOsc;
  $("#pair").classList.toggle("single", hasFan !== hasOsc);
  $("#fan-val").textContent = label("fan", s.fan);
  $("#osc-val").textContent = label("osc", s.oscillation);

  // Opções (só as que o modelo tem)
  for (const row of $$("[data-optional]")) {
    const key = row.dataset.optional;
    row.hidden = !has(f.optionalModes, key);
    const current = s.optionalMode || "off";
    row.classList.toggle("is-on", key === "windFree" ? current.startsWith("windFree") : current === key);
  }
  $("#row-light").hidden = f.light === "none";
  const lightKnown = f.light === "direct" && s.light != null;
  $("#sw-light").hidden = !lightKnown;
  $("#btn-light").hidden = lightKnown;
  $("#sw-light").classList.toggle("is-on", !!s.light);
  const windFree = has(f.optionalModes, "windFree");
  $("#row-night").hidden = f.light === "none" && !windFree;
  const parts = [];
  if (f.light !== "none") parts.push(T.nightParts.light);
  if (f.beep !== "none") parts.push(T.nightParts.beep);
  if (windFree) parts.push(T.nightParts.windFree);
  $("#night-hint").textContent = listText(parts);
  $("#options").hidden = $$("#options > .row").every((r) => r.hidden);

  // Configurações do ar
  $("#row-beep").hidden = f.beep === "none";
  $("#autoclean").hidden = !f.autoClean;
  $("#autoclean").classList.toggle("is-on", s.autoClean);
  $("#row-filter").hidden = !f.filter;
  $("#air-settings").hidden = $$("#air-settings > .row").every((r) => r.hidden);
  const fs = s.filterStatus;
  $("#filter").textContent = fs ? label("filterStatus", fs) : "--";
  $("#filter").classList.toggle("warn", !!fs && fs !== "normal");
  $("#filter-chip").hidden = !f.filter || !fs || fs === "normal" || snap.conn === "noToken";
  $("#filter-chip").textContent = fs === "replace" ? T.replaceFilter : T.washFilter;
}

function setTempDisplay(t) {
  const f = snap?.features;
  if (!f) return;
  $("#temp-val").textContent = deg(t, f.unit);
  const slider = $("#slider");
  if (document.activeElement !== slider || pendingTemp == null) slider.value = t;
  const span = f.tempMax - f.tempMin || 1;
  slider.style.setProperty("--pct", `${((t - f.tempMin) / span) * 100}%`);
}

function ago(ms) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 10) return T.now;
  if (s < 60) return T.secondsAgo(s);
  return T.minutesAgo(Math.round(s / 60));
}

function renderStatus() {
  const dot = $("#status-dot");
  const text = $("#status-text");
  const footer = $(".status");
  footer.classList.remove("err");
  text.title = "";
  dot.className = "dot";
  const offline = snap.state && !snap.state.online;
  switch (snap.conn) {
    case "ok":
      dot.classList.add(offline ? "warn" : "ok");
      text.textContent = offline
        ? T.offline
        : `${T.connected}${snap.updatedAt ? ` · ${T.updated} ${ago(snap.updatedAt)}` : ""}`;
      break;
    case "sending":
      dot.classList.add("sending");
      text.textContent = T.sending;
      break;
    case "offline":
      dot.classList.add("warn");
      text.textContent = T.noInternet;
      break;
    case "noToken":
      dot.classList.add("warn");
      text.textContent = T.signInToContinue;
      break;
    case "rateLimited":
      dot.classList.add("warn");
      text.textContent = snap.message || T.rateLimited;
      break;
    case "error":
      dot.classList.add("err");
      footer.classList.add("err");
      text.textContent = snap.message || T.error;
      text.title = snap.message || "";
      break;
    default:
      text.textContent = T.loading;
  }
}

// ---------- Ações ----------

async function act(type, value) {
  try {
    await invoke("act", { action: value === undefined ? { type } : { type, value } });
    return true;
  } catch (e) {
    console.warn(e);
    return false;
  }
}

/// Botões sem estado (luz, bipe, modo noite): só mostram se o envio deu certo.
async function fireAndFlash(btn, type, value) {
  btn.classList.remove("done", "fail");
  const ok = await act(type, value);
  btn.classList.add(ok ? "done" : "fail");
  setTimeout(() => btn.classList.remove("done", "fail"), 1400);
}

function chooseTemp(t) {
  const f = snap?.features;
  if (!f) return;
  t = Math.min(f.tempMax, Math.max(f.tempMin, Math.round((t - f.tempMin) / f.tempStep) * f.tempStep + f.tempMin));
  pendingTemp = t;
  setTempDisplay(t);
  clearTimeout(tempTimer);
  tempTimer = setTimeout(async () => {
    const value = pendingTemp;
    pendingTemp = null;
    if (snap?.state && snap.state.setpoint === value) return;
    await act("setpoint", value);
  }, 450);
}

// ---------- Folha de opções ----------

function openSheet(title, options, onPick) {
  $("#sheet-title").textContent = title;
  const box = $("#sheet-options");
  box.replaceChildren();
  for (const o of options) {
    const b = document.createElement("button");
    b.className = "opt" + (o.selected ? " sel" : "");
    const name = document.createElement("span");
    name.textContent = o.label;
    const radio = document.createElement("span");
    radio.className = "radio";
    b.append(name, radio);
    b.addEventListener("click", () => {
      closeSheet();
      if (!o.selected) onPick(o.value);
    });
    box.append(b);
  }
  $("#sheet-backdrop").hidden = false;
  $("#sheet").hidden = false;
  box.querySelector(".sel")?.focus();
}

function closeSheet() {
  $("#sheet-backdrop").hidden = true;
  $("#sheet").hidden = true;
}

const SHEETS = {
  mode: { title: () => T.mode, list: (f) => f.modes, group: "modes", key: "mode", action: "mode" },
  fan: { title: () => T.fanSpeed, list: (f) => f.fanModes, group: "fan", key: "fan", action: "fan" },
  osc: { title: () => T.airDirection, list: (f) => f.oscillationModes, group: "osc", key: "oscillation", action: "oscillation" },
};

function openValueSheet(kind) {
  const def = SHEETS[kind];
  const s = snap?.state;
  const f = snap?.features;
  if (!def || !s || !f) return;
  openSheet(
    def.title(),
    def.list(f).map((v) => ({ value: v, label: label(def.group, v), selected: s[def.key] === v })),
    (v) => act(def.action, v),
  );
}

async function openDeviceSheet() {
  try {
    const devices = await invoke("list_devices");
    openSheet(
      T.chooseDevice,
      devices.map((d) => ({ value: d.id, label: d.label, selected: d.selected })),
      async (id) => {
        snap = await invoke("select_device", { id });
        showView("main");
        render();
      },
    );
  } catch (e) {
    console.warn(e);
  }
}

// ---------- Telas ----------

function showView(name) {
  view = name;
  for (const v of ["main", "settings", "login"]) $(`#view-${v}`).hidden = v !== name;
  closeSheet();
  if (name === "login") {
    $("#btn-login-cancel").hidden = !snap || snap.conn === "noToken";
    $("#login-error").textContent = snap?.conn === "noToken" ? snap.message || "" : "";
    invoke("auth_info").then((info) => {
      $("#login-oauth").parentElement.hidden = !info.oauthAvailable;
      if (!info.oauthAvailable) $("#pat").open = true;
    });
  }
  if (name === "settings") {
    $("#device-current").textContent = snap?.deviceLabel || "";
    invoke("get_autostart").then((on) => $("#autostart").classList.toggle("is-on", on));
    invoke("auth_info").then((info) => {
      $("#account").textContent = { oauth: T.accountOauth, pat: T.accountPat, none: "--" }[info.method];
      $("#btn-login-oauth").hidden = !(info.oauthAvailable && info.method !== "oauth");
      $("#btn-logout").hidden = info.method === "none";
    });
  }
}

// ---------- Eventos ----------

function wire() {
  for (const el of $$("[data-sheet]")) el.addEventListener("click", () => openValueSheet(el.dataset.sheet));
  $("#sheet-backdrop").addEventListener("click", closeSheet);
  $("#sheet-cancel").addEventListener("click", closeSheet);

  $("#btn-power").addEventListener("click", () => snap?.state && act("power", !snap.state.power));
  const step = (dir) => {
    const s = snap?.state;
    const f = snap?.features;
    if (s && f) chooseTemp((pendingTemp ?? s.setpoint) + dir * f.tempStep);
  };
  $("#temp-up").addEventListener("click", () => step(1));
  $("#temp-down").addEventListener("click", () => step(-1));
  $("#slider").addEventListener("input", (e) => {
    pendingTemp = +e.target.value;
    setTempDisplay(pendingTemp);
    clearTimeout(tempTimer);
  });
  $("#slider").addEventListener("change", (e) => chooseTemp(+e.target.value));

  for (const row of $$("[data-optional]")) {
    row.addEventListener("click", () => {
      const s = snap?.state;
      if (!s) return;
      act("optionalMode", row.classList.contains("is-on") ? "off" : row.dataset.optional);
    });
  }
  $("#btn-light").addEventListener("click", (e) => fireAndFlash(e.currentTarget, "lightToggle"));
  $("#sw-light").addEventListener("click", () => snap?.state && act("light", !snap.state.light));
  for (const b of $$("[data-beep]")) b.addEventListener("click", () => fireAndFlash(b, "beep", b.dataset.beep === "on"));
  $("#btn-night").addEventListener("click", (e) => fireAndFlash(e.currentTarget, "nightMode"));
  $("#autoclean").addEventListener("click", () => snap?.state && act("autoClean", !snap.state.autoClean));

  $("#autostart").addEventListener("click", async (e) => {
    const el = e.currentTarget;
    try {
      el.classList.toggle("is-on", await invoke("set_autostart", { enabled: !el.classList.contains("is-on") }));
    } catch (err) {
      console.warn(err);
    }
  });
  $("#row-device").addEventListener("click", openDeviceSheet);
  $("#btn-settings").addEventListener("click", () => showView("settings"));
  $("#btn-back").addEventListener("click", () => showView("main"));
  $("#btn-quit").addEventListener("click", () => invoke("quit"));
  $("#btn-logout").addEventListener("click", async () => {
    snap = await invoke("logout");
    showView("login");
    render();
  });
  $("#btn-login-oauth").addEventListener("click", () => showView("login"));
  $("#btn-login-cancel").addEventListener("click", () => showView("main"));

  const loginDone = (result) => {
    snap = result;
    showView("main");
    render();
  };
  const loginError = (err) => {
    const msg = String(err);
    // Cancelar não é erro (a mensagem vem do backend, em pt ou en).
    $("#login-error").textContent = /^Login (cancelado|cancelled)$/.test(msg) ? "" : msg;
  };
  $("#btn-oauth").addEventListener("click", async () => {
    $("#btn-oauth").hidden = true;
    $("#oauth-wait").hidden = false;
    $("#login-error").textContent = "";
    try {
      loginDone(await invoke("login_oauth"));
    } catch (err) {
      loginError(err);
    } finally {
      $("#btn-oauth").hidden = false;
      $("#oauth-wait").hidden = true;
    }
  });
  $("#btn-oauth-cancel").addEventListener("click", () => invoke("cancel_login"));
  $("#btn-manual-open").addEventListener("click", () => {
    $("#login-error").textContent = "";
    invoke("login_manual_start").catch(loginError);
  });
  const submitWith = (form, command, field, key) =>
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      const btn = form.querySelector(".primary");
      btn.disabled = true;
      $("#login-error").textContent = "";
      try {
        loginDone(await invoke(command, { [key]: field.value }));
        field.value = "";
      } catch (err) {
        loginError(err);
      } finally {
        btn.disabled = false;
      }
    });
  submitWith($("#manual-form"), "login_manual_finish", $("#manual-input"), "input");
  submitWith($("#login-form"), "set_token", $("#token-input"), "token");

  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (!$("#sheet").hidden) return closeSheet();
    if (view === "settings") return showView("main");
    invoke("hide_popup");
  });
  document.addEventListener("contextmenu", (e) => e.preventDefault());
}

async function init() {
  applyStatic();
  for (const el of $$("[data-i18n-title]")) {
    el.title = T[el.dataset.i18nTitle];
    el.setAttribute("aria-label", T[el.dataset.i18nTitle]);
  }
  wire();
  if (tauri?.app?.getVersion) {
    tauri.app.getVersion().then((v) => ($("#version").textContent = `${T.appName} ${v}`)).catch(() => {});
  }

  await listen("ac-update", (e) => {
    snap = e.payload;
    render();
  });
  await listen("popup-shown", () => {
    closeSheet();
    if (view === "settings") showView("main");
    app.style.animation = "none";
    void app.offsetWidth;
    app.style.animation = "";
    render();
  });

  snap = await invoke("get_snapshot");
  render();
  setInterval(() => snap?.conn === "ok" && renderStatus(), 10_000);
}

init();
