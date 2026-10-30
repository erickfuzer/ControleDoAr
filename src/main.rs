//! Controle do Ar: controle remoto de ar-condicionado Samsung (SmartThings) na bandeja do Windows.
//!
//! Interface nativa em Slint (renderização por software, sem WebView). Módulos:
//! - `api`, `auth`: clientes HTTP da SmartThings (dispositivos e OAuth)
//! - `device`: o que cada modelo suporta, lido do status
//! - `state`: estado do ar e as ações que viram comandos
//! - `core`, `sync`, `session`: estado compartilhado, sincronização e credenciais
//! - `login`: login pelo navegador com volta automática pela página-ponte
//! - `ui`, `window`, `icon`, `hero`: interface, posicionamento do popup, ícone da bandeja e fundo do banner
//! - `secrets`, `config`, `winutil`: Gerenciador de Credenciais, configuração e APIs do Windows

// Sem janela de console no build final.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod auth;
mod config;
mod core;
mod device;
mod hero;
mod i18n;
mod icon;
mod login;
mod secrets;
mod session;
mod state;
mod sync;
mod ui;
mod window;
mod winutil;

use slint::winit_030::winit;

fn main() {
    // Uma cópia só: a segunda pede para a primeira abrir o popup e sai.
    let Some(show_event) = winutil::single_instance() else {
        return;
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("falha ao iniciar o runtime");
    let core = core::Core::new(rt.handle().clone());
    winutil::fix_autostart_path();

    // Janela criada já fora da barra de tarefas; renderização por software (leve, sem GPU).
    slint::BackendSelector::new()
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attrs| {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs.with_skip_taskbar(true)
        })
        .select()
        .expect("falha ao iniciar a interface");

    ui::start(core, show_event).expect("falha ao criar a interface");
    slint::run_event_loop_until_quit().expect("falha no laço de eventos");
    drop(rt);
}
