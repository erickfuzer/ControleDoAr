# Notas da API (testadas no aparelho real)

Modelo testado: Samsung WindFree 2024, `vid: DA-AC-RAC-01001`.

## Liga/desliga
`switch` `on`/`off`: funciona e o status atualiza em ~5 s.

## Luz do display

Nesse modelo a luz só responde ao `execute` com opções OCF:

```json
{ "commands": [ { "component": "main", "capability": "execute", "command": "execute",
  "arguments": ["mode/vs/0", { "x.com.samsung.da.options": ["Light_On"] }] } ] }
```

**Cada envio inverte a luz**, seja `Light_On` ou `Light_Off`: no uso real, as duas opções se comportaram igual
(2026-10-20). O primeiro teste (2026-10-11) tinha sugerido "semântica invertida" (`Light_On` apaga), mas aquela
sequência também é compatível com "inverter". Por isso o app trata a luz como um botão de inverter e, no modo noite,
não reenvia a luz se ele mesmo já a apagou.

O estado da luz **não pode ser lido**: `samsungce.airConditionerLighting` está em `custom.disabledCapabilities`,
o atributo `lighting` não acompanha a luz real e o `execute.data` volta `null`, mesmo depois de `refresh`.
O app lembra o último envio e considera que o display acende sempre que o ar liga
(confirmado em 2026-10-20: a luz apagada volta a acender ao religar, igual no app oficial).

Não confirmado: se `samsungce.airConditionerLighting` → `off` tem algum efeito (a API responde `COMPLETED`).

## Bipe (confirmado em 2026-10-20)

Também pelo `execute`: `["mode/vs/0", {"x.com.samsung.da.options": ["Volume_Mute"]}]` silencia e
`Volume_100` reativa. `samsungce.airConditionerBeep` está em `custom.disabledCapabilities`.

`COMPLETED` na resposta de `/commands` só significa que a nuvem aceitou, não que o aparelho executou.

## Outros modelos

O app não assume um modelo: `src-tauri/src/device.rs` lê do status o que o aparelho suporta.

- Modos, velocidades e direções: `supported*` (ou `available*`, se a primeira vier vazia).
- Faixa de temperatura: `coolingSetpointRange` ou `custom.thermostatSetpointControl`. A unidade vem do `unit` do setpoint (°C/°F).
- Luz e bipe: usa `samsungce.airConditionerLighting` / `samsungce.airConditionerBeep` quando a capability existe
  e **não** está em `custom.disabledCapabilities`; senão, o `execute` com opções OCF (como no modelo testado).
- Funções que não aparecem no status (WindFree, limpeza, filtro, umidade...) ficam escondidas na interface.

O status real do modelo testado (sem identificadores) está em `src-tauri/tests/fixtures/windfree-ar24.json`.

## Pendente
- Em quais modos o WindFree é aceito.

## OAuth (app criado em 2026-10-11)

- App "Controle do Ar" criado com `smartthings apps:create` (tipo `API_ONLY`), escopos
  `r:devices:* x:devices:* r:locations:*`. Client ID/secret ficam no `.env` (`ARCOND_CLIENT_ID`,
  `ARCOND_CLIENT_SECRET`) e entram no build pelo `build.rs`. O CI não usa esses segredos (compila sem OAuth).
- O PAT "fine grained" **não** serve para a CLI gerenciar apps (403 "Fine grained scope token not supported");
  a CLI precisa do login no navegador.
- Redirect URIs registradas: `http://localhost:47913/callback` e `https://httpbin.org/get`.
  **A SmartThings recusa `http://localhost` no `/oauth/authorize` (HTTP 403)**, apesar de a CLI aceitar o cadastro.
  Por isso o app usa `https://httpbin.org/get` e intercepta a navegação na janela de login, então o código não chega ao httpbin.
- Token: `POST https://api.smartthings.com/oauth/token` com Basic auth (client_id:secret).
  Access token dura 24h; refresh token é de uso único e expira em ~29 dias sem uso.
