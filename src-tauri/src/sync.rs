//! Sincronização com a nuvem: busca de status, envio de comandos com UI otimista
//! e respeito ao limite de requisições da SmartThings.

use crate::api::ApiError;
use crate::device::{Control, Features};
use crate::i18n::tr;
use crate::session::{has_credentials, with_token};
use crate::state::{AcState, Action};
use crate::{now_ms, publish, shared, Conn};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// O health (online/offline) muda pouco; não precisa ir junto em todo refresh.
const HEALTH_INTERVAL: Duration = Duration::from_secs(300);
/// Depois de um comando, a nuvem demora 1–2 s para refletir o novo estado.
pub const SETTLE_DELAY: Duration = Duration::from_secs(4);

pub fn set_conn(app: &AppHandle, conn: Conn, message: Option<String>) {
    {
        let s = shared(app);
        let mut i = s.inner.lock().unwrap();
        i.conn = Some(conn);
        i.message = message;
    }
    publish(app);
}

fn error_conn(e: &ApiError) -> Conn {
    match e {
        ApiError::Unauthorized => Conn::NoToken,
        ApiError::RateLimited(_) => Conn::RateLimited,
        ApiError::Network(_) => Conn::Offline,
        ApiError::Http(..) => Conn::Error,
    }
}

/// Registra o erro; num 429 bloqueia novas requisições pelo tempo pedido.
fn report_error(app: &AppHandle, e: &ApiError, message: String) {
    {
        let s = shared(app);
        let mut i = s.inner.lock().unwrap();
        if let ApiError::RateLimited(secs) = e {
            i.blocked_until = Some(Instant::now() + Duration::from_secs(*secs));
            refresh_after_block(app, *secs);
        }
        i.conn = Some(error_conn(e));
        i.message = Some(message);
    }
    publish(app);
}

/// Quando o bloqueio do 429 acabar, busca o status de novo (e limpa o aviso).
fn refresh_after_block(app: &AppHandle, secs: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(secs) + Duration::from_millis(300)).await;
        refresh(&app, true).await;
    });
}

/// Garante que há um aparelho escolhido: o configurado ou o primeiro ar-condicionado da conta.
async fn ensure_device(app: &AppHandle) -> Result<String, ApiError> {
    let s = shared(app);
    let configured = s.inner.lock().unwrap().cfg.device_id.clone();
    let client = s.client.clone();
    let devices = with_token(app, |t| {
        let c = client.clone();
        async move { c.devices(&t).await }
    })
    .await?;
    let pick = configured
        .as_ref()
        .and_then(|id| devices.iter().find(|d| &d.id == id))
        .or_else(|| devices.iter().find(|d| d.is_ac))
        .cloned();
    let Some(dev) = pick else {
        return Err(ApiError::Http(
            404,
            tr(
                "Nenhum ar-condicionado encontrado na conta",
                "No air conditioner found in this account",
            )
            .into(),
        ));
    };
    // Local e cômodo ("Casa - Quarto 1"); opcional, um PAT pode não ter o escopo.
    let place = match dev.location_id.clone() {
        Some(loc) => {
            let room = dev.room_id.clone();
            with_token(app, |t| {
                let (c, l, r) = (client.clone(), loc.clone(), room.clone());
                async move { c.place(&t, &l, r.as_deref()).await }
            })
            .await
            .ok()
            .filter(|p| !p.is_empty())
        }
        None => None,
    };
    let mut i = s.inner.lock().unwrap();
    i.device_label = Some(dev.label.clone());
    i.place = place;
    if i.cfg.device_id.as_deref() != Some(&dev.id) {
        i.cfg.device_id = Some(dev.id.clone());
        let _ = i.cfg.save(&s.cfg_path);
    }
    Ok(dev.id)
}

pub async fn refresh(app: &AppHandle, force: bool) {
    let s = shared(app);
    let (device, has_label, recent_cmd, blocked, health) = {
        let i = s.inner.lock().unwrap();
        (
            i.cfg.device_id.clone(),
            i.device_label.is_some(),
            i.last_command.map(|t| t.elapsed() < SETTLE_DELAY).unwrap_or(false),
            i.blocked_for().is_some(),
            i.health.filter(|(t, _)| t.elapsed() < HEALTH_INTERVAL).map(|(_, h)| h),
        )
    };
    // Não sobrescreve a UI otimista enquanto a nuvem ainda não refletiu o comando.
    if (recent_cmd && !force) || blocked {
        return;
    }
    if !has_credentials(app) {
        set_conn(app, Conn::NoToken, None);
        return;
    }
    let device = match device.filter(|_| has_label) {
        Some(d) => d,
        None => match ensure_device(app).await {
            Ok(d) => d,
            Err(e) => return report_error(app, &e, e.to_string()),
        },
    };
    let client = s.client.clone();
    let status = with_token(app, |t| {
        let (c, d) = (client.clone(), device.clone());
        async move { c.status(&t, &d).await }
    })
    .await;
    let health = match (health, &status) {
        (Some(h), _) => Ok(h),
        (None, Ok(_)) => {
            with_token(app, |t| {
                let (c, d) = (client.clone(), device.clone());
                async move { c.health(&t, &d).await }
            })
            .await
        }
        (None, Err(_)) => Err(ApiError::Unauthorized),
    };
    match status {
        Ok(v) => {
            {
                let mut i = s.inner.lock().unwrap();
                // Um comando chegou durante a requisição: a resposta pode estar velha.
                if !force && i.last_command.map(|t| t.elapsed() < SETTLE_DELAY).unwrap_or(false) {
                    return;
                }
                let online = match health {
                    Ok(h) => {
                        i.health = Some((Instant::now(), h));
                        h
                    }
                    Err(_) => i.health.map(|(_, h)| h).unwrap_or(true),
                };
                let features = Features::from_status(&v);
                let mut state = AcState::from_status(&v, online);
                if features.light != Control::Direct {
                    state.light = None;
                }
                // O display acende quando o ar liga (ligado pelo controle físico, por exemplo).
                if state.power && i.state.as_ref().is_some_and(|old| !old.power) {
                    i.display_dark = false;
                }
                i.state = Some(state);
                i.features = Some(features);
                i.conn = Some(Conn::Ok);
                i.message = None;
                i.updated_at = Some(now_ms());
                i.last_refresh = Some(Instant::now());
            }
            publish(app);
        }
        Err(e) => report_error(app, &e, e.to_string()),
    }
}

pub async fn act(app: &AppHandle, action: Action) -> Result<(), String> {
    let s = shared(app);
    if !has_credentials(app) {
        return Err(tr("Sem login", "Not signed in").into());
    }
    let (action, device, features, display_dark, prev, optimistic, gen) = {
        let mut i = s.inner.lock().unwrap();
        let (Some(device), Some(features)) = (i.cfg.device_id.clone(), i.features.clone()) else {
            return Err(tr("O aparelho ainda está carregando", "The device is still loading").into());
        };
        // Inverter a luz: usa o estado lido (capability direta) ou o último envio.
        let action = match action {
            Action::LightToggle => {
                let lit = i.state.as_ref().and_then(|st| st.light).unwrap_or(!i.display_dark);
                Action::Light(!lit)
            }
            a => a,
        };
        if let Some(secs) = i.blocked_for() {
            let msg = format!(
                "{} {secs} s",
                tr("Limite da API atingido, aguarde", "API rate limit reached, wait")
            );
            i.message = Some(msg.clone());
            drop(i);
            publish(app);
            return Err(msg);
        }
        i.cmd_gen += 1;
        let prev = i.state.clone();
        if let Some(st) = i.state.as_mut() {
            action.apply(st, &features);
        }
        i.last_command = Some(Instant::now());
        i.conn = Some(Conn::Sending);
        i.message = None;
        (
            action,
            device,
            features,
            i.display_dark,
            prev,
            i.state.clone(),
            i.cmd_gen,
        )
    };
    publish(app);

    let client = s.client.clone();
    let batches = action.plan(&features, display_dark);
    let mut sent = Ok(());
    for commands in batches {
        let r = with_token(app, |t| {
            let (c, d, cmds) = (client.clone(), device.clone(), commands.clone());
            async move { c.commands(&t, &d, cmds).await }
        })
        .await;
        // Guarda o primeiro erro, mas continua enviando os outros lotes,
        // exceto quando insistir não adianta (limite da API ou login inválido).
        if let Err(e) = r {
            let stop = matches!(e, ApiError::RateLimited(_) | ApiError::Unauthorized);
            if sent.is_ok() {
                sent = Err(e);
            }
            if stop {
                break;
            }
        }
    }
    match sent {
        Ok(()) => {
            {
                let mut i = s.inner.lock().unwrap();
                match action {
                    Action::Light(on) => i.display_dark = !on,
                    Action::NightMode if features.light != Control::None => i.display_dark = true,
                    Action::Power(true) => i.display_dark = false,
                    _ => {}
                }
            }
            set_conn(app, Conn::Ok, None);
            // Luz e bipe não têm estado para conferir.
            if matches!(action, Action::Light(_) | Action::Beep(_)) {
                return Ok(());
            }
            // Confere com a nuvem depois que ela assentar, uma vez só por rajada de cliques.
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(SETTLE_DELAY + Duration::from_millis(200)).await;
                let latest = shared(&app).inner.lock().unwrap().cmd_gen == gen;
                if latest {
                    refresh(&app, false).await;
                }
            });
            Ok(())
        }
        Err(e) => {
            let msg = format!("{}: {}", action.label(), e);
            {
                let mut i = s.inner.lock().unwrap();
                // Reverte só se ninguém mexeu no estado depois deste comando.
                if i.state == optimistic {
                    i.state = prev;
                }
                if let ApiError::RateLimited(secs) = e {
                    i.blocked_until = Some(Instant::now() + Duration::from_secs(secs));
                    refresh_after_block(app, secs);
                }
                i.conn = Some(match e {
                    ApiError::Unauthorized => Conn::NoToken,
                    ApiError::RateLimited(_) => Conn::RateLimited,
                    _ => Conn::Error,
                });
                i.message = Some(msg.clone());
            }
            publish(app);
            Err(msg)
        }
    }
}

pub fn spawn_act(app: &AppHandle, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = act(&app, action).await;
    });
}
