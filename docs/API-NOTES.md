# Notas da API (testadas no aparelho real)

Modelo testado: Samsung WindFree 2024, `vid: DA-AC-RAC-01001`.

## Liga/desliga
`switch` `on`/`off`: funciona e o status atualiza em ~5 s.

## Luz do display (confirmado em 2026-10-11)

**A semântica é invertida.** O nome da opção se refere à *função "luz desligada"*:

| Efeito desejado | Comando |
|---|---|
| Apagar o display | `execute` com `["mode/vs/0", {"x.com.samsung.da.options": ["Light_On"]}]` |
| Acender o display | `execute` com `["mode/vs/0", {"x.com.samsung.da.options": ["Light_Off"]}]` |

```json
{ "commands": [ { "component": "main", "capability": "execute", "command": "execute",
  "arguments": ["mode/vs/0", { "x.com.samsung.da.options": ["Light_On"] }] } ] }
```

NÃO funcionam (a API responde `COMPLETED`, mas o aparelho ignora):
- `samsungce.airConditionerLighting` → `off`
- `samsungce.airConditionerLighting` → `setLightingLevel("off")`

O atributo `samsungce.airConditionerLighting.lighting` **não é confiável**: não muda após os comandos
(a capability está em `custom.disabledCapabilities`). O `execute.data` também volta `null`, mesmo depois de
`refresh`. Resultado: o app **não consegue ler** o estado real da luz. Ele precisa lembrar o último estado
que enviou (e assumir que a luz acende sempre que o ar é ligado, já que o status mostrou `on` logo após o `switch on`).

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
- Bipe pelo `execute` (`Volume_Mute` / `Volume_100`): ainda não confirmado no aparelho.
- Testar se a luz apagada persiste ao desligar e ligar o ar.
- Em quais modos o WindFree é aceito.

## OAuth (app criado em 2026-10-11)

- App "Controle do Ar" criado com `smartthings apps:create` (tipo `API_ONLY`), escopos
  `r:devices:* x:devices:* r:locations:*`. Client ID/secret ficam no `.env` (`ARCOND_CLIENT_ID`,
  `ARCOND_CLIENT_SECRET`) e entram no build pelo `build.rs`. Para o CI, usar secrets com os mesmos nomes.
- O PAT "fine grained" **não** serve para a CLI gerenciar apps (403 "Fine grained scope token not supported");
  a CLI precisa do login no navegador.
- Redirect URIs registradas: `http://localhost:47913/callback` e `https://httpbin.org/get`.
  **A SmartThings recusa `http://localhost` no `/oauth/authorize` (HTTP 403)**, apesar de a CLI aceitar o cadastro.
  Por isso o app usa `https://httpbin.org/get` e intercepta a navegação na janela de login, então o código não chega ao httpbin.
- Token: `POST https://api.smartthings.com/oauth/token` com Basic auth (client_id:secret).
  Access token dura 24h; refresh token é de uso único e expira em ~29 dias sem uso.
