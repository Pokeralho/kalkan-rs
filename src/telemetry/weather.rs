//! Clima atual via Open-Meteo (sem chave de API), usando o `HttpClient` do
//! WinRT – nenhuma dependência HTTP extra. Desativado até que `weather_lat` e
//! `weather_lon` sejam definidos no `config.toml`.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Web::Http::HttpClient;

#[derive(Debug, Clone, Default)]
pub struct Weather {
    pub temp: f64,
    pub humidity: f64,
    pub wind: f64,
    pub code: u32,
}

pub fn describe(code: u32) -> &'static str {
    match code {
        0 => "Céu limpo",
        1 => "Poucas nuvens",
        2 => "Parcialmente nublado",
        3 => "Nublado",
        45 | 48 => "Neblina",
        51..=57 => "Garoa",
        61..=67 => "Chuva",
        71..=77 => "Neve",
        80..=82 => "Pancadas de chuva",
        85 | 86 => "Pancadas de neve",
        95..=99 => "Tempestade",
        _ => "—",
    }
}

pub type WeatherCell = Arc<Mutex<Option<Weather>>>;

fn fetch(lat: f64, lon: f64) -> anyhow::Result<Weather> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.3}&longitude={lon:.3}\
         &current=temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m"
    );
    let client = HttpClient::new()?;
    let body = client.GetStringAsync(&Uri::CreateUri(&HSTRING::from(url))?)?.get()?;
    let v: serde_json::Value = serde_json::from_str(&body.to_string_lossy())?;
    let c = &v["current"];
    Ok(Weather {
        temp: c["temperature_2m"].as_f64().unwrap_or(f64::NAN),
        humidity: c["relative_humidity_2m"].as_f64().unwrap_or(f64::NAN),
        wind: c["wind_speed_10m"].as_f64().unwrap_or(f64::NAN),
        code: c["weather_code"].as_u64().unwrap_or(999) as u32,
    })
}

/// Thread de baixa prioridade: atualiza a cada 15 min (5 min após falha).
pub fn spawn(lat: f64, lon: f64) -> WeatherCell {
    let cell: WeatherCell = Arc::default();
    let out = cell.clone();
    std::thread::Builder::new()
        .name("weather".into())
        .spawn(move || loop {
            let wait = match fetch(lat, lon) {
                Ok(w) => {
                    log::debug!("clima: {:.1}°C {}", w.temp, describe(w.code));
                    *out.lock().unwrap() = Some(w);
                    Duration::from_secs(15 * 60)
                }
                Err(e) => {
                    log::warn!("clima indisponível: {e:#}");
                    Duration::from_secs(5 * 60)
                }
            };
            std::thread::sleep(wait);
        })
        .expect("thread de clima");
    cell
}
