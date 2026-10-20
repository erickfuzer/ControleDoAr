// Backend falso para desenvolver a interface num navegador comum (sem Tauri).
// Não é usado pelo app. Parâmetros na URL para ver outros estados:
//   ?off      ar desligado
//   ?simple   modelo simples em °F, sem vento, sem WindFree
//   ?login    tela de login

const params = new URLSearchParams(location.search);
const listeners = {};

const features = params.has("simple")
  ? {
      modes: ["cool", "dry"], fanModes: [], oscillationModes: [], optionalModes: [],
      tempMin: 60, tempMax: 86, tempStep: 1, unit: "F",
      roomTemp: false, humidity: false, autoClean: false, filter: false, light: "direct", beep: "none",
    }
  : {
      modes: ["auto", "cool", "dry", "wind", "heat"],
      fanModes: ["auto", "low", "medium", "high", "turbo"],
      oscillationModes: ["fixed", "vertical", "horizontal", "all"],
      optionalModes: ["off", "sleep", "quiet", "smart", "speed", "windFree", "windFreeSleep"],
      tempMin: 16, tempMax: 30, tempStep: 1, unit: "C",
      roomTemp: true, humidity: true, autoClean: true, filter: true, light: "execute", beep: "execute",
    };

const snap = {
  state: {
    power: !params.has("off"), mode: "cool", setpoint: params.has("simple") ? 72 : 18,
    roomTemp: params.has("simple") ? null : 20, humidity: params.has("simple") ? null : 63,
    fan: "auto", oscillation: "fixed", optionalMode: "off", autoClean: false, filterStatus: "wash", online: true,
    light: params.has("simple") ? true : null,
  },
  features,
  conn: params.has("login") ? "noToken" : "ok",
  message: null,
  updatedAt: Date.now() - 20000,
  deviceLabel: "Ar-condicionado",
  place: "Casa - Quarto 1",
};

const keys = { power: "power", mode: "mode", setpoint: "setpoint", fan: "fan", oscillation: "oscillation", optionalMode: "optionalMode", autoClean: "autoClean" };

function emit(event, payload) {
  for (const cb of listeners[event] || []) cb({ payload });
}

export async function invoke(cmd, args) {
  switch (cmd) {
    case "act": {
      const { type, value } = args.action;
      if (keys[type]) snap.state[keys[type]] = value;
      if (type === "nightMode") snap.state.optionalMode = "windFree";
      if (type === "light" && snap.state.light != null) snap.state.light = value;
      setTimeout(() => emit("ac-update", structuredClone(snap)), 0);
      return null;
    }
    case "auth_info":
      return { oauthAvailable: true, method: "oauth" };
    case "list_devices":
      return [
        { id: "a", label: "Ar-condicionado", selected: true },
        { id: "b", label: "Ar da sala", selected: false },
      ];
    case "logout":
      snap.conn = "noToken";
      return structuredClone(snap);
    case "get_autostart":
      return true;
    default:
      return structuredClone(snap);
  }
}

export async function listen(event, cb) {
  (listeners[event] ||= []).push(cb);
}
