<div align="center">

# ❄️ Controle do Ar

Controle remoto do ar-condicionado **Samsung** na bandeja do Windows, usando a API oficial da **SmartThings**. Um clique no ícone e o controle aparece ao lado dele, mais rápido que pegar o celular.

![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-24C8DB?style=for-the-badge&logo=tauri&logoColor=white)
![JavaScript](https://img.shields.io/badge/JavaScript-F7DF1E?style=for-the-badge&logo=javascript&logoColor=black)
![SmartThings](https://img.shields.io/badge/SmartThings-15BFFF?style=for-the-badge&logo=smartthings&logoColor=white)
![Windows](https://img.shields.io/badge/Windows_10_e_11-0078D4?style=for-the-badge)

<img src="docs/screenshot-on.png" alt="Controle com o ar ligado em resfriamento, temperatura, modo, vento e WindFree" width="300">
<img src="docs/screenshot-off.png" alt="Controle com o ar desligado" width="300">
<br>
<sub>O controle com o ar ligado e desligado</sub>
<br><br>

[![Como compilar](https://img.shields.io/badge/▶_Como_compilar-3C7CF8?style=for-the-badge)](#como-compilar)

</div>

## Funcionalidades

- **Ícone na bandeja com a temperatura alvo**, na cor do modo (azul no resfriamento, laranja no aquecimento etc.) e legível mesmo em 16 px
- **Clique esquerdo:** abre um controle no estilo do app SmartThings, com ligar, modo, temperatura, intensidade e direção do vento, WindFree, Silencioso e Máx. (rápido)
- **Clique direito:** menu rápido com ligar, temperatura ±, modo, modo noite e luz do display
- **Luz do display:** acende e apaga a luz que o app oficial esconde em alguns modelos. Vira um interruptor quando o modelo informa o estado da luz, e um botão quando não informa
- **Modo noite:** apaga a luz, silencia o bipe e liga o WindFree num clique só
- **Se adapta a cada modelo:** modos, velocidades, faixa e unidade de temperatura (°C ou °F) vêm do próprio aparelho, e o que o modelo não tem fica escondido
- **Vários aparelhos:** se a conta tiver mais de um ar, você escolhe qual controlar nas configurações
- **Resposta imediata:** a mudança aparece na hora e é desfeita se a nuvem recusar. O estado atualiza a cada minuto, e o app espera sozinho quando bate no limite de requisições da API
- **Português ou inglês**, conforme o idioma do Windows
- **Leve:** alguns MB de Rust mais o WebView2 do sistema, sem Electron

## Como funciona

O app conversa com a `api.smartthings.com` usando a sua conta SmartThings. O login é feito por OAuth numa janela do próprio app, que é anônima e não guarda nada depois de fechada. Os tokens ficam no **Gerenciador de Credenciais do Windows** e são renovados automaticamente. Também dá para entrar com um token pessoal (PAT).

Tudo o que o controle mostra vem do status do aparelho (`supportedAcModes`, `supportedAcFanModes`, faixa do setpoint, `custom.disabledCapabilities` etc.). Para a luz do display e o bipe, o app usa a capability do próprio modelo quando ela está ativa e, quando não está, o comando OCF `execute`, o mesmo que o app da Samsung usa. As descobertas sobre a API, incluindo algumas surpresas, estão em [docs/API-NOTES.md](docs/API-NOTES.md).

Testado num Samsung WindFree 2024 (`DA-AC-RAC-01001`). Outros modelos Samsung que expõem as capabilities padrão de ar-condicionado da SmartThings devem funcionar. Se testar no seu, conte como foi.

## Como compilar

Não há instaladores prontos para baixar: o app OAuth carrega um client secret, então cada pessoa compila com o seu.

**Requisitos:** Windows 10 ou 11, [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 20 ou mais novo e o runtime do WebView2 (já vem no Windows 11).

### 1. Crie o seu app OAuth na SmartThings (só uma vez)

Instale a SmartThings CLI:

```bash
npm install -g @smartthings/cli
```

Abra o [`docs/smartthings-app.json`](docs/smartthings-app.json), troque o `appName` por um nome único e crie o app:

```bash
smartthings apps:create -i docs/smartthings-app.json
```

A CLI abre o navegador para você entrar na conta e mostra um **OAuth Client Id** e um **Client Secret**. Copie o `.env.example` para `.env` e preencha os dois valores. O `.env` é ignorado pelo Git e deve continuar assim.

> [!NOTE]
> **Por que o redirecionamento é o httpbin:** a SmartThings recusa redirecionamentos para `http://localhost`, então o app usa `https://httpbin.org/get`. Ele intercepta esse redirecionamento dentro da própria janela de login, e o código de autorização nunca chega ao httpbin. Já no login pelo navegador (necessário para entrar com a conta Google), a página só mostra o código para você colar no app.

Também dá para pular este passo: sem o app OAuth, o login pede um [token pessoal (PAT)](https://account.smartthings.com/tokens), que expira em 24 horas.

### 2. Rode ou gere o instalador

```bash
npm install
npm run dev     # desenvolvimento, com a interface recarregando sozinha
npm run build   # instalador em src-tauri/target/release/bundle/nsis/
```

O instalador é por usuário (não pede administrador) e baixa o WebView2 se o PC não tiver. Ele não tem assinatura digital, então o SmartScreen do Windows avisa na primeira vez.

> [!WARNING]
> **Não publique instaladores gerados com o seu client secret.** Qualquer pessoa conseguiria extraí-lo do executável. Sozinho ele não dá acesso a nenhuma conta, mas deixa outras pessoas usarem a cota de instalações do seu app OAuth.

### Para desenvolver

- **Testes:** `cargo test --manifest-path src-tauri/Cargo.toml`. Incluem o status real de um aparelho, sem identificadores, em `src-tauri/tests/fixtures`.
- **Só a interface:** sirva a pasta `src/` com qualquer servidor estático e abra o `index.html`. Ele usa um backend falso (`dev-mock.js`). Adicione `?off`, `?simple` (modelo em °F sem vento nem WindFree) ou `?login` na URL para ver outros estados.

## Limitações

- Em alguns modelos a API não informa o estado da luz do display e do bipe, então eles aparecem como botões (como o "Acender/apagar" da luz), não como interruptores.
- O login com a conta Google não funciona em janelas embutidas. Use a opção de entrar pelo navegador.
- Só funciona no Windows: a bandeja, o Gerenciador de Credenciais e o posicionamento da janela são específicos dele.

## Estrutura

```
ControleDoAr/
├── src/                    # interface do controle (HTML, CSS e JS, sem framework nem bundler)
│   └── dev-mock.js         # backend falso para mexer na interface no navegador
├── src-tauri/
│   ├── src/
│   │   ├── device.rs       # o que cada modelo suporta, lido do status
│   │   ├── state.rs        # estado do aparelho e as ações que viram comandos da API
│   │   ├── sync.rs         # atualização do status, comandos imediatos e limite de requisições
│   │   ├── session.rs      # OAuth (com renovação), PAT e janela de login
│   │   ├── auth.rs         # cliente OAuth da SmartThings
│   │   ├── api.rs          # cliente REST da SmartThings
│   │   ├── secrets.rs      # Gerenciador de Credenciais do Windows
│   │   ├── tray.rs         # ícone e menu da bandeja
│   │   ├── popup.rs        # janela do controle
│   │   └── icon.rs         # desenha a temperatura no ícone da bandeja
│   └── tests/fixtures/     # status real de um aparelho, para os testes
├── docs/                   # notas da API, modelo do app OAuth e prints
├── .env.example            # modelo do .env com as credenciais do app OAuth
├── LICENSE
└── README.md
```

## Créditos

Não é afiliado à Samsung. SmartThings é uma marca da Samsung Electronics.

Ícones baseados no [Lucide](https://lucide.dev) (licença ISC). Desenvolvido com o auxílio do [Claude](https://claude.ai) (Anthropic).

## Licença

[MIT](LICENSE): use, modifique e distribua à vontade.

<div align="center">
<sub>Feito por <a href="https://github.com/erickfuzer">Erick Fuzer</a></sub>
</div>
