# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).

## [0.3.1] — 2026-11-08

### Corrigido
- Se o `theme.toml` sumisse por um instante (git checkout, sincronização do OneDrive, editores que salvam apagando e recriando o arquivo), o display ficava preso na tela "ERRO NO TEMA". Agora o tema atual continua na tela e o arquivo é lido de novo a cada 1 s até voltar.
- O hot-reload parava de funcionar se a pasta do tema fosse removida e recriada. Além do watcher do sistema, a pasta do tema exibido é conferida uma vez por segundo.

## [0.3.0] — 2026-11-08

### Adicionado
- **Música:**
  - capa do álbum (`album_art`), com cantos arredondados e crossfade;
  - posição/duração/progresso da faixa (`media_position_str`, `media_progress`…);
  - **letra sincronizada** (`lyrics`, via LRCLIB) no estilo karaokê, com rolagem suave.
- **Fundo `album`:** capa desfocada com crossfade. `blur` também funciona em imagens estáticas.
- **Paleta extraída da capa** (`$album_vibrant`, `$album_light`, `$album_dark`, `$album_dominant`): o tema se recolore a cada música.
- **Tema Now Playing e modo música automático** (`music_mode`, `music_theme`, `music_return_s`), com toggle na bandeja e no Studio.
- `$nome/NN`: cor da paleta com opacidade.
- `KALKAN_DEMO_MEDIA=1`: faixa fictícia para criar temas e documentação.
- **Studio:** ferramentas Capa e Letra, modelo "Player completo", fundo Capa, seção Música na aba Display.

### Alterado
- Camadas estáticas da base da pilha passam a ser rasterizadas uma única vez (até 5× menos custo por atualização).
- O letreiro (`marquee`) só pede quadros novos quando o texto realmente rola. O modo música fica em ~0,1% de CPU.
- `--preview` espera as transições terminarem antes de capturar.
- Tema padrão de uma instalação nova: Dark Hub.
- README em inglês, com versão em português (`README.pt-BR.md`).

## [0.2.0] — 2026-11-08

### Adicionado
- **Kalkan Studio** (Tauri 2): editor visual no estilo canvas. Pré-visualização pelo motor real, alças de redimensionamento, guias magnéticas, edição de pontos, camadas, desfazer/refazer, inspector, modelos prontos, paleta, abas Display e Dados.
- **Expressões** em placeholders, `metric`, `visible_if` e `color_rules`, com funções (`min`, `max`, `clamp`, `if`…) e variáveis de tempo (`t`, `hour`…).
- **Comportamentos em todas as camadas:** `visible_if`, `color_rules`, `animation` (`pulse`, `blink`, `spin`, `marquee`, `fade_in`, `slide_up`), `glow` e `layer_opacity`.
- **Novas camadas:**
  - `shape`: retângulo, elipse, polígono regular, estrela, polígono livre e linha, com degradês, rotação, repetição radial, traços extras e preenchimento guiado por métrica;
  - `cores`: grade de uso por núcleo.
- **Texto:** contorno, degradê vertical, maiúsculas e letreiro rolante.
- **Paleta do tema** (`[palette]`, `$nome`) e fonte padrão (`meta.font`).
- **Tela de inicialização Dark Hub** (`assets/splash/`), editável no Studio, com `splash` e `splash_ms` no config.
- Temas **Dark Hub** e **Neon Shapes**.
- `--record` para gravar a animação de um tema em GIF.
- **Bandeja:** submenu de FPS, abrir o Studio, aplicação ao vivo de mudanças externas no `config.toml`, lista de temas atualizada sozinha.
- Inverter e espelhar a imagem por software (`flip_vertical` / `flip_horizontal`).

### Alterado
- **GPU:** o backend padrão passou a ser D3DKMT/DXGI. A RAM do serviço caiu de ~40 MB para ~20 MB; NVML continua disponível como opção.
- **FPS** padrão de 24, configurável de 1 a 30.
- **Conversão RGB565** ~10× mais rápida (~8 µs por quadro).
- **Glow e opacidade** processam só a área ocupada pela camada.
- O driver descarta quadros idênticos ao anterior.

### Documentado
- Engenharia reversa completa do protocolo da `LcdComm.dll` em `docs/PROTOCOL.md`.

## [0.1.0] — 2026-11-08

### Adicionado
- Serviço em Rust:
  - driver via `LcdComm.dll` com triple buffer, keep-alive e reconexão;
  - telemetria (CPU, GPU, RAM, disco, rede, mídia, clima);
  - vídeo via Media Foundation;
  - GIF/WebP/APNG;
  - motor de temas TOML com hot-reload;
  - bandeja do sistema.
- Temas Cyberpunk e Minimal Clean.
