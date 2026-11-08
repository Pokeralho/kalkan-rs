# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).

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
