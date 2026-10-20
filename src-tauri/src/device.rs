//! O que cada ar-condicionado suporta, lido do próprio status.
//!
//! Os modelos Samsung expõem as mesmas capabilities com listas diferentes de valores
//! (modos, velocidades, faixa de temperatura), e algumas funções só funcionam pelo
//! comando `execute` do protocolo OCF. Tudo que a interface mostra sai daqui.

use serde::Serialize;
use serde_json::Value;

/// Como mandar a luz do display e o bipe.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Control {
    /// Sem suporte.
    None,
    /// Capability própria (`samsungce.airConditionerLighting` / `samsungce.airConditionerBeep`).
    Direct,
    /// Pelo `execute` com opções OCF, quando a capability está desativada no modelo.
    Execute,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Features {
    pub modes: Vec<String>,
    pub fan_modes: Vec<String>,
    /// Vazio: o modelo não tem direção do vento.
    pub oscillation_modes: Vec<String>,
    /// Modos especiais (WindFree, Silencioso, Máx. rápido...).
    pub optional_modes: Vec<String>,
    pub temp_min: f64,
    pub temp_max: f64,
    pub temp_step: f64,
    /// "C" ou "F".
    pub unit: String,
    pub room_temp: bool,
    pub humidity: bool,
    pub auto_clean: bool,
    pub filter: bool,
    pub light: Control,
    pub beep: Control,
}

impl Features {
    pub fn has_optional(&self, mode: &str) -> bool {
        self.optional_modes.iter().any(|m| m == mode)
    }

    /// Modo noite faz sentido se houver ao menos luz ou WindFree para mexer.
    pub fn night_mode(&self) -> bool {
        self.light != Control::None || self.has_optional("windFree")
    }

    pub fn clamp_temp(&self, t: f64) -> f64 {
        let step = if self.temp_step > 0.0 { self.temp_step } else { 1.0 };
        let snapped = self.temp_min + ((t - self.temp_min) / step).round() * step;
        snapped.clamp(self.temp_min, self.temp_max)
    }

    pub fn from_status(v: &Value) -> Self {
        let m = &v["components"]["main"];
        let has = |cap: &str| m.get(cap).is_some();
        let list = |cap: &str, attr: &str| -> Vec<String> {
            m[cap][attr]["value"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default()
        };
        // Usa a primeira lista não vazia (alguns modelos só preenchem a "available").
        let first = |options: &[(&str, &str)]| -> Vec<String> {
            options
                .iter()
                .map(|(c, a)| list(c, a))
                .find(|l| !l.is_empty())
                .unwrap_or_default()
        };
        let disabled = list("custom.disabledCapabilities", "disabledCapabilities");
        let usable = |cap: &str| has(cap) && !disabled.iter().any(|d| d == cap);
        let control = |cap: &str| {
            if usable(cap) {
                Control::Direct
            } else if has("execute") {
                Control::Execute
            } else {
                Control::None
            }
        };

        let unit = m["thermostatCoolingSetpoint"]["coolingSetpoint"]["unit"]
            .as_str()
            .unwrap_or("C")
            .to_string();
        let (def_min, def_max) = if unit == "F" { (60.0, 86.0) } else { (16.0, 30.0) };
        let range = &m["thermostatCoolingSetpoint"]["coolingSetpointRange"]["value"];
        let ctl = &m["custom.thermostatSetpointControl"];
        let temp_min = range["minimum"]
            .as_f64()
            .or(ctl["minimumSetpoint"]["value"].as_f64())
            .unwrap_or(def_min);
        let temp_max = range["maximum"]
            .as_f64()
            .or(ctl["maximumSetpoint"]["value"].as_f64())
            .unwrap_or(def_max);
        let temp_step = range["step"].as_f64().filter(|s| *s > 0.0).unwrap_or(1.0);

        let mut modes = first(&[
            ("airConditionerMode", "supportedAcModes"),
            ("airConditionerMode", "availableAcModes"),
        ]);
        if modes.is_empty() && has("airConditionerMode") {
            modes = ["auto", "cool", "dry", "wind", "heat"].map(String::from).to_vec();
        }

        Self {
            modes,
            fan_modes: first(&[
                ("airConditionerFanMode", "supportedAcFanModes"),
                ("airConditionerFanMode", "availableAcFanModes"),
            ]),
            oscillation_modes: if usable("fanOscillationMode") {
                first(&[
                    ("fanOscillationMode", "supportedFanOscillationModes"),
                    ("fanOscillationMode", "availableFanOscillationModes"),
                ])
            } else {
                vec![]
            },
            optional_modes: if usable("custom.airConditionerOptionalMode") {
                first(&[
                    ("custom.airConditionerOptionalMode", "supportedAcOptionalMode"),
                    ("custom.airConditionerOptionalMode", "availableAcOptionalMode"),
                ])
            } else {
                vec![]
            },
            temp_min,
            temp_max: temp_max.max(temp_min),
            temp_step,
            unit,
            room_temp: m["temperatureMeasurement"]["temperature"]["value"].is_number(),
            humidity: m["relativeHumidityMeasurement"]["humidity"]["value"].is_number(),
            auto_clean: usable("custom.autoCleaningMode"),
            filter: usable("custom.dustFilter"),
            light: control("samsungce.airConditionerLighting"),
            beep: control("samsungce.airConditionerBeep"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/windfree-ar24.json")).unwrap()
    }

    #[test]
    fn le_o_windfree_2024() {
        let f = Features::from_status(&fixture());
        assert_eq!(f.modes, ["auto", "cool", "dry", "wind", "heat"]);
        assert_eq!(f.fan_modes, ["auto", "low", "medium", "high", "turbo"]);
        assert_eq!(f.oscillation_modes, ["fixed", "vertical", "horizontal", "all"]);
        assert!(f.has_optional("windFree") && f.has_optional("quiet") && f.has_optional("speed"));
        assert_eq!(
            (f.temp_min, f.temp_max, f.temp_step, f.unit.as_str()),
            (16.0, 30.0, 1.0, "C")
        );
        assert!(f.room_temp && f.humidity && f.auto_clean && f.filter);
        // Luz e bipe estão em disabledCapabilities nesse modelo: só pelo execute.
        assert_eq!(f.light, Control::Execute);
        assert_eq!(f.beep, Control::Execute);
        assert!(f.night_mode());
    }

    #[test]
    fn modelo_simples_sem_extras() {
        let v: Value = serde_json::from_str(
            r#"{"components":{"main":{
                "switch":{"switch":{"value":"on"}},
                "airConditionerMode":{"airConditionerMode":{"value":"cool"},"supportedAcModes":{"value":["cool","dry"]}},
                "thermostatCoolingSetpoint":{"coolingSetpoint":{"value":72,"unit":"F"}},
                "samsungce.airConditionerLighting":{"lighting":{"value":"on"}}
            }}}"#,
        )
        .unwrap();
        let f = Features::from_status(&v);
        assert_eq!(f.modes, ["cool", "dry"]);
        assert!(f.fan_modes.is_empty() && f.oscillation_modes.is_empty() && f.optional_modes.is_empty());
        assert_eq!((f.temp_min, f.temp_max, f.unit.as_str()), (60.0, 86.0, "F"));
        assert!(!f.room_temp && !f.humidity && !f.auto_clean);
        assert_eq!(f.light, Control::Direct);
        assert_eq!(f.beep, Control::None);
        assert!(f.night_mode());
    }

    #[test]
    fn arredonda_para_o_passo_e_a_faixa() {
        let mut f = Features::from_status(&fixture());
        assert_eq!(f.clamp_temp(31.0), 30.0);
        assert_eq!(f.clamp_temp(15.0), 16.0);
        f.temp_step = 0.5;
        assert_eq!(f.clamp_temp(20.3), 20.5);
    }
}
