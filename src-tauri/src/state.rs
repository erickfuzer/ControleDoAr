//! Estado do ar e as ações que o app sabe executar.

use crate::api::{cmd, exec_option};
use crate::device::{Control, Features};
use crate::i18n::tr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AcState {
    pub power: bool,
    pub mode: String,
    pub setpoint: f64,
    pub room_temp: Option<f64>,
    pub humidity: Option<f64>,
    pub fan: Option<String>,
    pub oscillation: Option<String>,
    /// off | sleep | quiet | smart | speed | windFree | windFreeSleep
    pub optional_mode: Option<String>,
    pub auto_clean: bool,
    /// Ex.: "normal", "wash", "replace"
    pub filter_status: Option<String>,
    pub online: bool,
    /// Luz do display, só quando o modelo informa (capability direta). Com o `execute`
    /// a API não diz se está acesa.
    pub light: Option<bool>,
}

impl AcState {
    pub fn from_status(v: &Value, online: bool) -> Self {
        let m = &v["components"]["main"];
        let s = |cap: &str, attr: &str| m[cap][attr]["value"].as_str().map(String::from);
        let n = |cap: &str, attr: &str| m[cap][attr]["value"].as_f64();
        Self {
            power: s("switch", "switch").as_deref() == Some("on"),
            mode: s("airConditionerMode", "airConditionerMode").unwrap_or_else(|| "cool".into()),
            setpoint: n("thermostatCoolingSetpoint", "coolingSetpoint").unwrap_or(24.0),
            room_temp: n("temperatureMeasurement", "temperature"),
            humidity: n("relativeHumidityMeasurement", "humidity"),
            fan: s("airConditionerFanMode", "fanMode"),
            oscillation: s("fanOscillationMode", "fanOscillationMode"),
            optional_mode: s("custom.airConditionerOptionalMode", "acOptionalMode"),
            auto_clean: s("custom.autoCleaningMode", "autoCleaningMode").as_deref() == Some("on"),
            filter_status: s("custom.dustFilter", "dustFilterStatus"),
            online,
            light: s("samsungce.airConditionerLighting", "lighting").map(|v| v != "off"),
        }
    }
}

/// Ações vindas do popup ou do menu da bandeja.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum Action {
    Power(bool),
    Mode(String),
    Setpoint(f64),
    Fan(String),
    Oscillation(String),
    OptionalMode(String),
    AutoClean(bool),
    /// Sem leitura de estado: a API não informa se a luz está acesa.
    Light(bool),
    /// Inverte a luz. Vira `Light(..)` em `sync::act`, que sabe (ou supõe) o estado atual.
    LightToggle,
    /// Sem leitura de estado, igual à luz.
    Beep(bool),
    NightMode,
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

impl Action {
    pub fn commands(&self, f: &Features) -> Vec<Value> {
        match self {
            Action::Power(on) => vec![cmd("switch", on_off(*on), vec![])],
            Action::Mode(m) => vec![cmd("airConditionerMode", "setAirConditionerMode", vec![json!(m)])],
            Action::Setpoint(t) => {
                let t = f.clamp_temp(*t);
                // Inteiro quando o passo é 1, para não mandar "24.0".
                let v = if t.fract() == 0.0 { json!(t as i64) } else { json!(t) };
                vec![cmd("thermostatCoolingSetpoint", "setCoolingSetpoint", vec![v])]
            }
            Action::Fan(m) => vec![cmd("airConditionerFanMode", "setFanMode", vec![json!(m)])],
            Action::Oscillation(o) => vec![cmd("fanOscillationMode", "setFanOscillationMode", vec![json!(o)])],
            Action::OptionalMode(o) => {
                vec![cmd(
                    "custom.airConditionerOptionalMode",
                    "setAcOptionalMode",
                    vec![json!(o)],
                )]
            }
            Action::AutoClean(on) => {
                vec![cmd(
                    "custom.autoCleaningMode",
                    "setAutoCleaningMode",
                    vec![json!(on_off(*on))],
                )]
            }
            Action::Light(on) => match f.light {
                Control::Direct => vec![cmd("samsungce.airConditionerLighting", on_off(*on), vec![])],
                // Semântica invertida, confirmada no aparelho: "Light_On" liga a função "luz apagada".
                Control::Execute => vec![exec_option(if *on { "Light_Off" } else { "Light_On" })],
                Control::None => vec![],
            },
            Action::Beep(on) => match f.beep {
                Control::Direct => vec![cmd("samsungce.airConditionerBeep", on_off(*on), vec![])],
                Control::Execute => vec![exec_option(if *on { "Volume_100" } else { "Volume_Mute" })],
                Control::None => vec![],
            },
            Action::NightMode => self.batches(f).concat(),
            // Resolvido antes de chegar aqui (ver `sync::act`).
            Action::LightToggle => vec![],
        }
    }

    /// Envios separados: no modo noite, um comando recusado (ex.: bipe) não pode
    /// impedir os outros. A luz vai primeiro porque é o que mais importa à noite.
    pub fn batches(&self, f: &Features) -> Vec<Vec<Value>> {
        match self {
            Action::NightMode => {
                let mut parts = vec![Action::Light(false).commands(f)];
                if f.has_optional("windFree") {
                    parts.push(Action::OptionalMode("windFree".into()).commands(f));
                }
                parts.push(Action::Beep(false).commands(f));
                parts.into_iter().filter(|b| !b.is_empty()).collect()
            }
            _ => vec![self.commands(f)].into_iter().filter(|b| !b.is_empty()).collect(),
        }
    }

    /// Os envios de fato. No modo noite com a luz já apagada (`display_dark`), não manda a luz:
    /// em modelos onde o comando alternativo inverte em vez de apagar, isso a acenderia.
    pub fn plan(&self, f: &Features, display_dark: bool) -> Vec<Vec<Value>> {
        let mut batches = self.batches(f);
        if matches!(self, Action::NightMode) && display_dark && f.light != Control::None && !batches.is_empty() {
            batches.remove(0);
        }
        batches
    }

    /// Aplica o efeito esperado no estado local (UI otimista).
    pub fn apply(&self, s: &mut AcState, f: &Features) {
        match self {
            Action::Power(on) => s.power = *on,
            Action::Mode(m) => s.mode = m.clone(),
            Action::Setpoint(t) => s.setpoint = f.clamp_temp(*t),
            Action::Fan(m) => s.fan = Some(m.clone()),
            Action::Oscillation(o) => s.oscillation = Some(o.clone()),
            Action::OptionalMode(o) => s.optional_mode = Some(o.clone()),
            Action::AutoClean(on) => s.auto_clean = *on,
            Action::NightMode if f.has_optional("windFree") => s.optional_mode = Some("windFree".into()),
            Action::Light(on) => {
                if s.light.is_some() {
                    s.light = Some(*on);
                }
            }
            Action::NightMode | Action::LightToggle | Action::Beep(_) => {}
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Action::Power(true) => tr("Ligar", "Turn on"),
            Action::Power(false) => tr("Desligar", "Turn off"),
            Action::Mode(_) => tr("Modo", "Mode"),
            Action::Setpoint(_) => tr("Temperatura", "Temperature"),
            Action::Fan(_) => tr("Vento", "Fan speed"),
            Action::Oscillation(_) => tr("Direção do vento", "Air direction"),
            Action::OptionalMode(_) => tr("Modo especial", "Special mode"),
            Action::AutoClean(_) => tr("Limpeza automática", "Auto clean"),
            Action::Light(_) | Action::LightToggle => tr("Luz do display", "Display light"),
            Action::Beep(_) => tr("Bipe", "Beep"),
            Action::NightMode => tr("Modo noite", "Night mode"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn features() -> Features {
        let v: Value = serde_json::from_str(include_str!("../tests/fixtures/windfree-ar24.json")).unwrap();
        Features::from_status(&v)
    }

    #[test]
    fn luz_pelo_execute_com_semantica_invertida() {
        let c = Action::Light(false).commands(&features());
        assert_eq!(c[0]["capability"], "execute");
        assert_eq!(c[0]["arguments"][1]["x.com.samsung.da.options"][0], "Light_On");
    }

    #[test]
    fn luz_direta_quando_o_modelo_aceita() {
        let mut f = features();
        f.light = Control::Direct;
        let c = Action::Light(false).commands(&f);
        assert_eq!(
            (c[0]["capability"].as_str(), c[0]["command"].as_str()),
            (Some("samsungce.airConditionerLighting"), Some("off"))
        );
    }

    #[test]
    fn modo_noite_luz_primeiro_e_pula_o_que_nao_existe() {
        let mut f = features();
        let b = Action::NightMode.batches(&f);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0][0]["arguments"][1]["x.com.samsung.da.options"][0], "Light_On");
        f.optional_modes.clear();
        f.beep = Control::None;
        assert_eq!(Action::NightMode.batches(&f).len(), 1);
    }

    #[test]
    fn modo_noite_nao_reenvia_a_luz_ja_apagada() {
        let f = features();
        let light = |b: &Vec<Vec<Value>>| {
            b.iter().flatten().any(|c| {
                c["capability"] == "execute"
                    && c["arguments"][1]["x.com.samsung.da.options"][0]
                        .as_str()
                        .unwrap_or("")
                        .starts_with("Light")
            })
        };
        assert!(light(&Action::NightMode.plan(&f, false)));
        let dark = Action::NightMode.plan(&f, true);
        assert!(!light(&dark));
        assert_eq!(dark.len(), 2); // WindFree e bipe continuam
    }

    #[test]
    fn temperatura_fica_na_faixa_e_vai_inteira() {
        let c = Action::Setpoint(35.0).commands(&features());
        assert_eq!(c[0]["arguments"][0], json!(30));
    }

    #[test]
    fn le_o_estado() {
        let v: Value = serde_json::from_str(include_str!("../tests/fixtures/windfree-ar24.json")).unwrap();
        let s = AcState::from_status(&v, true);
        assert_eq!(s.mode, "cool");
        assert_eq!(s.fan.as_deref(), Some("auto"));
        assert_eq!(s.filter_status.as_deref(), Some("wash"));
    }
}
