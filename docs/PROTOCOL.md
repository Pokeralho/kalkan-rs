# Protocolo do display (engenharia reversa da `LcdComm.dll`)

> Documentação obtida por análise estática da `LcdComm.dll` (x64, 86 KB) instalada pelo software oficial. Nenhum arquivo do fabricante faz parte deste repositório.

## Hardware

| Item | Valor |
|---|---|
| USB | VID `0x1B80` · PID `0xB550` (CDC-ACM → porta COM) |
| Tela | 320 × 240, RGB565 **big-endian** |
| Quadro | 320 × 240 × 2 = **153.600 bytes** |
| Transferência | blocos múltiplos de 512 bytes (USB high-speed) |

## API exportada

```c
uint32_t GetDLLVersion(void);                                   // 0x10005
void     SetUsbID(const uint32_t* list, uint32_t n);            // [0x1B80B550]
void     UsbDeviceEvent(void);                                  // (re)enumera e conecta
void     GetDevices(uint8_t out[128]);                          // out[0] > 0 = conectado
bool     PlayOneRGBFrame(uint8_t idx, const uint8_t* rgb, uint32_t size); // size % 512 == 0
uint32_t SetDeviceBrightness(uint32_t idx, uint32_t b);         // 0..100
uint32_t SetDeviceDegree(uint32_t idx, uint32_t deg);           // 0..3 (×90°)
uint32_t SetDeviceParam(uint32_t idx, uint32_t deg, uint32_t b, uint32_t save);
uint32_t SetDeviceSaveParam / SetDeviceSleepTimeout / SetDeviceConnectTimeout / SetDeviceDirect
uint32_t GetDeviceParam / GetDeviceVersion
uint32_t StartOTA(...)                                          // atualização de firmware (não usado)
```

A convenção é `stdcall`, que no x64 é a ABI padrão. Os índices vão de 0 a 15.

## Transporte

As funções `CreateFileA("\\.\COMx")` são chamadas com E/S assíncrona (overlapped), 8N1. Os outros parâmetros da abertura:

| Parâmetro | Valor |
|---|---|
| `SetupComm` | 1024 / 1024 |
| `PurgeComm` | todos |
| Timeouts | `ReadIntervalTimeout = MAXDWORD` |

O porte é localizado pela SetupAPI, enumerando a classe `Ports` e filtrando pelo VID/PID na instance ID.

## Handshake

1. Envia `" HWCX-TECH-VRFY0"` e depois `" HWCX-TECH-VRFY1"`, pacotes de 16 bytes com o primeiro byte trocado por `0x01`.
2. Envia um **desafio** de 16 bytes:
   - byte 0 = `0x01`;
   - bytes 1–15 aleatórios, exceto:
     - `b[4] = (b[2] + b[3]) ^ b[1]`;
     - `b[8] = (b[5] + b[6]) ^ b[7]`;
     - `b[9] = ~b[10]`;
     - `b[11] = −Σ b[1..=10]` (checksum).
3. Lê **17 bytes**. Eles precisam começar com `"HWCXTECH-"`; em seguida vem a resolução em ASCII, por exemplo `320x240`, que o log interno registra como `verify 320 x 240`.
4. Envia `[0x10, 0x00, …]` e lê **32 bytes**:
   - UUID, versão e parâmetros salvos (rotação, brilho);
   - a DLL calcula um CRC32 (polinômio `0x04C11DB7`) sobre esses bytes para identificar o aparelho.

## Envio de quadro (`PlayOneRGBFrame`)

```
offset = 0, primeiro = 1
enquanto offset < 153600:
    repete por até 1 s:
        envia  [0x05, primeiro, 0 × 14]         (16 bytes)
        lê     espaco: u16 (little-endian)      (2 bytes) — espaço livre no buffer do aparelho
    até espaco >= 512
    n = maior múltiplo de 512 <= espaco, limitado ao que falta
    envia  rgb[offset .. offset+n]
    offset += n, primeiro = 0
```

## Comandos (`SetDeviceDirect`)

Todos os comandos são pacotes de 16 bytes. O valor `0xFF` significa "manter o atual".

| Comando | Bytes |
|---|---|
| Rotação | `[0x08, deg, 0xFF, 0…]` |
| Brilho | `[0x08, 0xFF, brilho, 0…]` |
| Timeout de conexão | `[0x08, 0xFF, 0xFF, valor, 0…]` |
| Timeout de suspensão | `[0x08, 0xFF, 0xFF, 0x00, valor, 0…]` |
| Gravar parâmetros | `[0x08, 0xFF, 0xFF, 0…, b[15] = 0x53]` |
| Rotação + brilho (+ gravar) | `[0x08, deg, brilho, 0…, b[15] = 0x53 se gravar]` |

## Log de depuração da DLL

Um `config.ini` ao lado da DLL com o conteúdo abaixo liga o log interno em `debug.txt`:

```ini
[config]
debug=9
```

## Por que manter a DLL

Medindo o tempo de CPU da thread do driver separado em kernel e usuário, a 20 fps:

| Modo | % de um núcleo |
|---|---|
| Kernel | 4–6% |
| Usuário | 0,3–1,5% |

O tempo de kernel é o `usbser.sys` transferindo cerca de 3 MB/s, e cresce com os bytes enviados. O código da própria DLL responde por menos de 0,1% da CPU total. Reimplementar o protocolo não traria ganho relevante. O que reduz o custo é enviar menos bytes:

- limitar o FPS (`max_fps`, padrão 24);
- descartar quadros idênticos ao anterior, o que o driver já faz.
