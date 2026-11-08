# Arquitetura

```
kalkan-rs/
├── Cargo.toml              workspace: serviço (raiz) + studio/
├── src/
│   ├── main.rs             CLI, logger, instância única, orquestração das threads
│   ├── lib.rs              biblioteca compartilhada com o Studio
│   ├── config.rs           config.toml (com recarga externa)
│   ├── driver/
│   │   ├── mod.rs          triple buffer, comandos, medição de CPU por thread
│   │   └── lcd_comm.rs     LcdComm.dll via libloading, keep-alive, reconexão, descarte de quadros repetidos
│   ├── telemetry/          coleta a 1 Hz
│   │   ├── cpu.rs          sysinfo (uso por núcleo) + PDH (clock real) + temperatura
│   │   ├── gpu.rs          D3DKMT/DXGI (padrão) · NVML (opcional) · PDH
│   │   ├── hwmon.rs        LibreHardwareMonitor/OpenHardwareMonitor via WMI
│   │   ├── memory.rs       RAM, swap, disco (ocupação + PDH de throughput)
│   │   ├── network.rs      download/upload
│   │   ├── media.rs        WinRT GlobalSystemMediaTransportControlsSessionManager
│   │   ├── weather.rs      Open-Meteo via WinRT HttpClient
│   │   └── pdh.rs          wrapper do Performance Data Helper
│   ├── media/
│   │   ├── mod.rs          fundos (sólido, estático, animado, vídeo) e ajuste cover/contain/stretch
│   │   ├── gif.rs          GIF/WebP/APNG com cache RGB565 ou streaming
│   │   └── video.rs        Media Foundation Source Reader → RGB32 → recorte 320×240
│   ├── renderer/
│   │   ├── canvas.rs       framebuffer ARGB pré-multiplicado, primitivas AA, glow, conversão RGB565 (AVX2)
│   │   ├── expr.rs         linguagem de expressões
│   │   ├── widgets.rs      camadas, comportamentos, fontes, templates
│   │   └── engine.rs       tema, compositor, hot-reload, splash, loop de renderização, preview/GIF
│   └── ui/tray.rs          bandeja (tray-icon) + loop de mensagens Win32
├── studio/                 Kalkan Studio (Tauri 2)
│   ├── src/main.rs         comandos: render binário, documento ⇄ TOML, assets, config
│   └── ui/                 HTML/CSS/JS sem build step
└── assets/
    ├── themes/             temas
    ├── splash/             tela de inicialização (tema comum)
    └── fonts/              fontes compartilhadas (opcional)
```

## Threads do serviço

```
 telemetry (1 Hz) ──► TelemetryHub (Arc<Snapshot> + versão)
                              │
 decoders (vídeo/GIF) ──► fila ─┤
                              ▼
 render ── compõe o quadro ──► TripleBuffer ──► driver ── PlayOneRGBFrame ──► USB
   ▲                                              ▲
   └── RenderCmd ◄── tray (thread principal) ──► DriverCmd
```

| Thread | O que faz |
|---|---|
| **principal** | Loop de mensagens Win32 e menu da bandeja; a cada 1 s confere se o `config.toml` mudou por fora (Studio) e aplica as diferenças |
| **telemetry** | Coleta tudo em 1 Hz e publica um `Snapshot` imutável |
| **render** | Acorda só quando precisa: novo quadro de vídeo/GIF, telemetria nova, mudança de segundo, animação ou hot-reload |
| **driver** | Consome sempre o quadro mais recente; reenvia a cada 1 s como keep-alive, reconecta quando o cabo volta e descarta quadros idênticos |
| **decoders** | Vídeo e animações em threads próprias, com fila de pré-buffer |

## Pipeline de renderização

1. **Fundo:** o fundo (cor, imagem, quadro de GIF ou de vídeo) é copiado para o canvas e, se pedido, escurecido.
2. **Camada de widgets (overlay):** fica em cache, em ARGB pré-multiplicado. Só é redesenhada quando os dados ou o relógio mudam, ou quando há animação. Assim, um vídeo a 24 fps apenas recompõe o cache sobre cada quadro.
3. **Composição** do overlay sobre o fundo.
4. **Conversão** para RGB565 big-endian já com a orientação (espelhamento) aplicada, em ~8 µs com AVX2.
5. **Publicação** no triple buffer, sem nunca esperar pelo USB.

### Comportamentos por camada

Cada camada tem um conteúdo (`Kind`) e comportamentos (`BehaviorRt`). Antes de desenhar:

1. **Visibilidade:** `visible_if` é avaliado.
2. **Cor:** `color_rules` produz uma cor de substituição.
3. **Animação:** ajusta opacidade, rotação, deslocamento ou letreiro conforme `t`.
4. **Camada temporária:** se houver glow, opacidade ou deslocamento, a camada é desenhada em um canvas `scratch` e composta depois. O glow aplica 3 passadas de box blur apenas na caixa ocupada pela camada.

## Telemetria de GPU

| Backend | Fontes | RAM |
|---|---|---|
| `auto` / D3DKMT | `D3DKMTQueryAdapterInfo` (temperatura, RPM, clock), DXGI (VRAM total), PDH `GPU Engine`/`GPU Adapter Memory` (uso, VRAM usada) | — |
| `nvml` | NVIDIA Management Library (inclui watts e fan %) | +19 MB |
| `pdh` | Somente PDH | — |

## Studio

O Studio usa a biblioteca `kalkan_rs` diretamente:

- **Documento:** o tema é convertido em uma lista única de camadas com `type`. Ao salvar, `z` vira o índice de cada camada na lista e o documento volta a ser `ThemeFile` / TOML.
- **`render`:** devolve um bloco binário com as caixas de cada camada (para seleção e alças) seguido do RGBA 320×240, a cerca de 30 fps.
- **Campos `_editor`** (oculto, travado) existem só no editor e não vão para o TOML.
