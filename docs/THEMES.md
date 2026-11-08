# Referência de temas

Um tema é uma pasta dentro de `assets/themes/` que contém um `theme.toml`, mais os arquivos que ele usa (imagens, vídeos, fontes). Coordenadas são em pixels da tela de **320×240**, com origem no canto superior esquerdo.

O arquivo é recarregado sozinho quando salvo (hot-reload). Se houver um erro, a mensagem aparece no próprio display. Para gerar uma imagem do tema sem o hardware:

```bash
kalkan-rs --preview meu_tema
```

- [Estrutura do arquivo](#estrutura-do-arquivo)
- [Cores e paleta](#cores-e-paleta)
- [Fundo](#fundo)
- [Camadas](#camadas)
  - [`text`](#text) · [`radial_gauge`](#radial_gauge) · [`bar`](#bar) · [`sparkline`](#sparkline) · [`cores`](#cores) · [`shape`](#shape) · [`rect`](#rect) · [`image`](#image) · [`album_art`](#album_art) · [`lyrics`](#lyrics)
- [Música: capa, letra e cores adaptáveis](#música-capa-letra-e-cores-adaptáveis)
- [Comportamentos (todas as camadas)](#comportamentos-todas-as-camadas)
- [Expressões](#expressões)
- [Placeholders de texto](#placeholders-de-texto)
- [Métricas disponíveis](#métricas-disponíveis)
- [Tela de inicialização](#tela-de-inicialização)
- [Dicas de desempenho](#dicas-de-desempenho)

## Estrutura do arquivo

```toml
[meta]
name = "Nome exibido"
author = "Você"
description = "…"
fps = 24                  # teto de FPS do tema (o menor entre este e o global vale)
font = "bahnschrift.ttf"  # fonte padrão dos textos

[palette]                 # opcional
accent = "#38E1F0"

[background]
type = "solid"
color = "#000000"

[[widgets.text]]          # quantas camadas quiser, de qualquer tipo
# …
```

**Ordem de desenho:** as camadas são desenhadas pelo campo `z`, de forma crescente: as de `z` maior ficam por cima. Quando duas camadas têm o mesmo `z`, vale a ordem de tipo `shape` → `rect` → `image` → `sparkline` → `cores` → `bar` → `radial_gauge` → `text`, e depois a ordem de declaração. O Kalkan Studio grava `z` igual à posição de cada camada na lista.

## Cores e paleta

Todo campo de cor aceita:

| Formato | Exemplo |
|---|---|
| `#RGB` / `#RGBA` | `"#0FC"`, `"#0FC8"` |
| `#RRGGBB` / `#RRGGBBAA` | `"#00FFCC"`, `"#00FFCC80"` (alfa no fim) |
| `$nome` | `"$accent"`, cor definida em `[palette]` |
| `$nome/NN` | `"$accent/40"`, cor da paleta com 40% de opacidade |

A paleta permite recolorir o tema inteiro mudando um único valor. Uma entrada pode apontar para outra: `alias = "$accent"`.

## Fundo

```toml
[background]
type = "video"     # video | gif | static | solid | album | auto (deduz pela extensão)
path = "bg.mp4"    # relativo à pasta do tema
fit = "cover"      # cover (preenche e recorta) | contain (tarjas) | stretch (estica)
dim = 0.35         # escurece o fundo 0..1 para destacar os widgets
color = "#000000"  # usado por type = "solid" (e por "album" enquanto não há capa)
blur = 0           # desfoque em px (static e album; album usa 14 se omitido)
```

| Tipo | Formatos | Observações |
|---|---|---|
| `video` | MP4 (H.264/HEVC), MOV, WMV, AVI, MKV | Decodificado pelo Windows (Media Foundation) em thread própria, em loop contínuo |
| `gif` | GIF, WebP animado, APNG | Os quadros ficam em cache na RAM (`anim_cache_mb`); acima disso, streaming |
| `static` | PNG, JPG, WebP | Redimensionado uma única vez ao carregar |
| `solid` | — | Apenas a cor |
| `album` | — | Capa da música tocando, desfocada, com crossfade de 0,8 s na troca de faixa |

## Camadas

### `text`

```toml
[[widgets.text]]
text = "CPU {cpu_temp:.1}°C"   # placeholders, ver abaixo
font = "segoeui.ttf"           # arquivo .ttf/.otf: pasta do tema, assets/fonts ou C:\Windows\Fonts
size = 18                      # tamanho em pixels (em)
x = 160
y = 120
align = "center"               # left | center | right (em relação a x)
valign = "middle"              # top | middle | bottom (em relação a y)
color_hex = "#FFFFFF"
opacity = 1.0
letter_spacing = 0             # px extras entre letras
max_width = 200                # trunca com "…" (ou rola com animation = "marquee")
shadow = "#000000AA"           # sombra deslocada 1px
outline = "#000000"            # contorno
outline_width = 1.5
gradient = ["#FFFFFF", "#38E1F0"]  # degradê vertical (substitui color_hex)
uppercase = false
```

Glifos que faltam na fonte escolhida (▶, ↓, °…) são buscados automaticamente em fontes de reserva (Segoe UI, Segoe UI Symbol).

### `radial_gauge`

```toml
[[widgets.radial_gauge]]
metric = "cpu_temp"            # métrica ou expressão
min = 30
max = 95
center_x = 80
center_y = 100
radius = 50
stroke_width = 8
start_angle = -135             # graus; 0 = topo, sentido horário
end_angle = 135                # 0..360 desenha um anel completo
color_gradient = ["#00FF9C", "#FFE600", "#FF2B4F"]
color_mode = "arc"             # arc = degradê ao longo do arco | value = cor única pelo valor
track_color = "#FFFFFF22"
rounded_caps = true
smooth = true                  # anima suavemente entre as leituras
```

### `bar`

```toml
[[widgets.bar]]
metric = "ram_pct"
min = 0
max = 100
x = 10
y = 200
width = 140
height = 8
orientation = "horizontal"     # horizontal | vertical (enche de baixo para cima)
radius = 4
color_gradient = ["#34D399", "#F87171"]   # cor conforme o valor
color_hex = "#FFFFFF"          # alternativa: cor única
track_color = "#FFFFFF22"
smooth = true
```

### `sparkline`

Gráfico de linha do histórico (1 amostra por segundo, até 120 s).

```toml
[[widgets.sparkline]]
metric = "cpu_usage"           # precisa ser uma métrica com histórico (ver tabela)
x = 10
y = 150
width = 300
height = 50
seconds = 30
min = 0                        # omita min/max para escala automática
max = 100
color_hex = "#22D3EE"
stroke_width = 1.5
fill_opacity = 0.25            # área translúcida sob a curva
```

### `cores`

Grade com uma barra por núcleo lógico da CPU.

```toml
[[widgets.cores]]
x = 8
y = 140
width = 304
height = 40
columns = 0                    # núcleos por linha (0 = todos numa linha)
gap = 2
orientation = "vertical"       # vertical | horizontal
radius = 2
color_gradient = ["#38E1F0", "#7B61FF", "#FF3B5C"]
track_color = "#FFFFFF0C"
```

### `shape`

Formas vetoriais com anti-aliasing. Os pontos são **normalizados** (0..1) dentro da caixa `x/y/width/height`, então a forma escala junto com a caixa.

```toml
[[widgets.shape]]
kind = "star"                  # rect | ellipse | regular | star | polygon | line
x = 120
y = 60
width = 80
height = 80
rotation = 0                   # graus, em torno do centro
repeat = 1                     # repetição radial: N cópias giradas (mandalas, flocos)

# por tipo
sides = 5                      # regular / star
inner_ratio = 0.45             # star
corner_radius = 8              # rect
points = [[0, 1], [0.5, 0], [1, 1]]            # polygon / line
paths = [[[0.3, 0.2], [0.5, 0.4], [0.7, 0.2]]] # traços extras (só contorno)

# preenchimento
fill = "#FFFFFF"
fill_gradient = ["#FFE600", "#FF2BD6"]   # sobrepõe fill
gradient_type = "linear"       # linear | radial
gradient_angle = 90            # 0 = esquerda→direita, 90 = topo→base
opacity = 1.0

# contorno
stroke = "#FFFFFF"
stroke_width = 2               # line/path: 2 por padrão

# preenchimento "líquido" guiado por métrica
metric = "cpu_usage"
min = 0
max = 100
fill_direction = 270           # 270 = de baixo para cima; 0 = da esquerda
```

Exemplo: o floco da tela de inicialização é **uma única camada**. São uma haste (`points`) e duas forquilhas (`paths`), repetidas 6× com `repeat = 6`.

### `rect`

Retângulo simples, útil como painel.

```toml
[[widgets.rect]]
x = 0
y = 0
width = 320
height = 24
radius = 6
color_hex = "#00000088"
```

### `image`

```toml
[[widgets.image]]
path = "logo.png"              # relativo à pasta do tema
x = 4
y = 4
width = 32                     # opcional; mantém a proporção se só um for dado
height = 32
opacity = 1.0
```

### `album_art`

Capa da música tocando, com cantos arredondados e crossfade na troca de faixa.

```toml
[[widgets.album_art]]
x = 14
y = 16
width = 104
height = 104
radius = 10
opacity = 1.0
placeholder = "#FFFFFF14"      # cor exibida sem capa (omita para não desenhar nada)
glow = "#000000B0"             # brilho preto = sombra
glow_radius = 7
```

### `lyrics`

Letra sincronizada no estilo karaokê: a linha atual fica destacada e a troca de linha rola suavemente. Linhas longas encolhem até 70% do tamanho e, se ainda não couberem, são truncadas.

```toml
[[widgets.lyrics]]
x = 12
y = 132
width = 296
height = 100
font = "seguisb.ttf"
size = 16                      # linha atual
size_other = 12.5              # demais linhas (padrão 80% de size)
color_hex = "#FFFFFF"          # linha atual
color_dim = "#FFFFFF8C"        # demais linhas (esmaecem nas bordas da área)
lines = 3
line_gap = 7
align = "center"               # left | center | right
empty_text = "♪"               # sem letra (aceita placeholders)
```

## Música: capa, letra e cores adaptáveis

**Dados da faixa:** vêm da sessão de mídia do Windows (Spotify, navegadores, Apple Music…). A posição avança suavemente entre as leituras.

**Letras:** são buscadas no [LRCLIB](https://lrclib.net) quando a faixa muda. Apenas título, artista, álbum e duração são enviados, e o recurso pode ser desligado com `lyrics = false`. Quando só existe a letra sem tempos, as linhas avançam proporcionalmente ao progresso da música.

**Cores da capa:** a cada capa nova, o kalkan-rs extrai uma paleta. Qualquer campo de cor pode usar estas entradas, e o tema inteiro muda de cor a cada música:

| Cor | Uso sugerido |
|---|---|
| `$album_vibrant` (ou `$album`) | Destaques: barra de progresso, ícones |
| `$album_dominant` | Cor predominante da capa |
| `$album_dark` | Fundos e painéis |
| `$album_light` | Textos secundários |

Sem música tocando, essas cores usam valores neutros. Um tema também pode definir `album_*` na própria `[palette]` para substituí-las.

**Modo música automático:** configurado por `music_mode`, `music_theme` e `music_return_s`. Ao começar a tocar, o display troca para o tema de música (padrão `now_playing`) e volta ao tema normal depois de `music_return_s` segundos pausado. Se você escolher outro tema manualmente durante a música, a escolha é respeitada até a música parar.

> **Demonstração sem música:** rode com a variável `KALKAN_DEMO_MEDIA=1` (serviço, `--preview`, `--record` ou Studio) para usar uma faixa fictícia com capa e letra de exemplo.

## Comportamentos (todas as camadas)

Estas chaves funcionam em **qualquer** tipo de camada:

| Chave | Exemplo | Efeito |
|---|---|---|
| `visible_if` | `"media_playing == 1"` | A camada só aparece quando a expressão é verdadeira |
| `color_rules` | `[{ when = "cpu_temp >= 85", color = "#FF3B5C" }]` | A primeira regra verdadeira substitui a cor principal da camada |
| `animation` | `"pulse"` | Ver tabela abaixo |
| `animation_speed` | `0.5` | Multiplicador de velocidade |
| `glow` | `"$accent"` | Brilho difuso ao redor da camada |
| `glow_radius` | `6` | Raio do brilho (1–24) |
| `layer_opacity` | `0.6` | Opacidade da camada inteira |

| Animação | Efeito |
|---|---|
| `pulse` | Opacidade oscila suavemente |
| `blink` | Pisca (útil com `visible_if` para alertas) |
| `spin` | Gira formas continuamente (90°/s × velocidade) |
| `marquee` | Texto maior que `max_width` rola como um letreiro |
| `fade_in` | Surge ao carregar o tema |
| `slide_up` | Surge deslizando para cima |

Exemplo de alerta que só aparece quando a CPU esquenta, piscando:

```toml
[[widgets.text]]
text = "CPU QUENTE  {cpu_temp:.0}°C"
x = 160
y = 12
align = "center"
color_hex = "#FF3B5C"
visible_if = "cpu_temp >= 90"
animation = "blink"
```

## Expressões

As expressões funcionam em placeholders, em `metric`, em `visible_if` e em `color_rules.when`.

| Recurso | Sintaxe |
|---|---|
| Aritmética | `+ - * / %` e parênteses |
| Comparação | `< <= > >= == !=` (o resultado vale 1 ou 0) |
| Lógica | `&& \|\| !` |
| Funções | `min(a,b,…)`, `max`, `abs`, `round(x)`, `round(x, casas)`, `floor`, `ceil`, `sqrt`, `pow`, `sin`, `cos`, `clamp(x,min,max)`, `lerp(a,b,t)`, `if(cond, a, b)` |
| Constantes | `true`, `false`, `pi` |

Se uma métrica estiver indisponível, a expressão inteira fica indisponível e o texto mostra `--`. As exceções são `||` e `if`, que avaliam apenas o lado necessário.

```text
cpu_temp * 1.8 + 32                  Fahrenheit
ram_used_gb / ram_total_gb * 100     porcentagem calculada
max(cpu_temp, gpu_temp)              o mais quente
hour >= 19 || hour < 6               noite
clamp(t / 2 * 100, 0, 100)           barra de 0 a 100% em 2 s
sin(t * 3) * 0.5 + 0.5               oscilação
```

**Variáveis extras:**

| Variável | Valor |
|---|---|
| `t` | Segundos desde que o tema foi carregado |
| `hour`, `minute`, `second` | Relógio |
| `day`, `month`, `year` | Data |
| `weekday_num` | Dia da semana, 1 = segunda |

## Placeholders de texto

| Forma | Resultado |
|---|---|
| `{cpu_temp}` | `57` (sem casas decimais) |
| `{cpu_temp:.1}` | `57.3` |
| `{cpu_temp:5.1}` | `" 57.3"` (largura mínima 5) |
| `{cpu_temp*1.8+32:.0}` | Expressão formatada |
| `{time}` / `{time:%H:%M:%S}` | Hora no formato [strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) |
| `{date}` / `{date:%d/%m}` | Data |
| `{weekday}` | Nome do dia |
| `{media_title}` | Texto (ver tabela) |
| `{{` / `}}` | Chaves literais |

## Métricas disponíveis

**Numéricas:** usadas em expressões, gauges, barras e placeholders.

| Grupo | Métricas |
|---|---|
| CPU | `cpu_usage`, `cpu_core_N` (núcleo N), `cpu_cores`, `cpu_temp`, `cpu_power`, `cpu_clock` (MHz), `cpu_clock_ghz` |
| GPU | `gpu_usage`, `gpu_temp`, `gpu_hotspot`, `gpu_clock`, `gpu_power`, `gpu_fan_pct`, `gpu_fan_rpm`, `gpu_vram_used`, `gpu_vram_total` (MB), `gpu_vram_used_gb`, `gpu_vram_total_gb`, `gpu_vram_pct` |
| Memória | `ram_used`, `ram_total` (MB), `ram_used_gb`, `ram_total_gb`, `ram_pct`, `swap_used`, `swap_pct` |
| Disco | `disk_pct`, `disk_used_gb`, `disk_total_gb`, `disk_read_mb`, `disk_write_mb` (MB/s) |
| Rede | `net_down_kb`, `net_up_kb`, `net_down_mb`, `net_up_mb` (por segundo) |
| Música | `media_playing`, `media_active` (0/1), `media_position`, `media_duration`, `media_remaining` (s), `media_progress` (0–100), `lyrics_available`, `album_art_available` |
| Outros | `weather_temp`, `weather_humidity`, `weather_wind` |

**Texto:** usadas apenas em placeholders.

`cpu_name`, `gpu_name`, `net_down`, `net_up`, `disk_read`, `disk_write` (com unidade automática), `media_title`, `media_artist`, `media_album`, `media_state`, `media_icon` (▶ / ❚❚ / ■), `media_position_str`, `media_duration_str`, `media_remaining_str` (`1:23`, `-2:05`), `lyrics_line`, `lyrics_prev`, `lyrics_next`, `weather_desc`.

**Com histórico** (aceitas pelo `sparkline`): `cpu_usage`, `cpu_temp`, `cpu_power`, `cpu_clock`, `gpu_usage`, `gpu_temp`, `gpu_power`, `ram_pct`, `net_down_kb`, `net_up_kb`, `disk_read_mb`, `disk_write_mb`.

> **Sensores sem administrador:** o Windows não expõe Tdie, watts de CPU nem hotspot/RPM de GPU por APIs públicas. Com o **LibreHardwareMonitor** aberto, esses valores são lidos automaticamente. O backend `nvml` (configuração `gpu_backend`) traz watts e fan % de placas NVIDIA.

## Tela de inicialização

A tela de inicialização é um tema comum, guardado em `assets/splash/`. Ela aparece por `splash_ms` milissegundos quando o serviço inicia. Edite pelo Kalkan Studio, escolhendo **Tela de inicialização** na lista de temas, ou direto no arquivo. A variável `t` permite sincronizar animações e barras de carregamento.

## Dicas de desempenho

- **Camadas estáticas** no fundo da pilha (painéis, véus, formas sem métrica nem animação) são desenhadas uma única vez e reutilizadas.
- **Temas estáticos** só são redesenhados quando algum valor muda, o que custa praticamente zero de CPU.
- **Fundos animados** custam CPU proporcional ao FPS, a maior parte dela no driver USB do Windows. Ajuste `max_fps`; 15–24 já fica fluido.
- **`glow` e animações:** `glow` processa só a área da camada, e animações contínuas (`pulse`, `spin`, `marquee`) redesenham as camadas a cada quadro. Use com moderação em temas que ficam ligados o tempo todo.
- **GIFs grandes:** acima de `anim_cache_mb` são decodificados em streaming em vez de ficarem em cache.
