//! `engram login` / `logout` / `whoami` / `health`: akun hub tim tanpa membuka vault.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;

use memory_hub::client::{HubClient, HubCreds, LoginFlow, open_browser};

fn print_out(json_mode: bool, v: &serde_json::Value, human: impl FnOnce() -> String) {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    } else {
        println!("{}", human());
    }
}

/// Hub bawaan: `--hub`, lalu `[repo_share] hub_url` di memory.toml, lalu login terakhir.
fn default_hub(config: &Path, hub: Option<&str>) -> Option<String> {
    hub.map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .or_else(|| std::fs::read_to_string(config).ok().and_then(|s| memory_core::MemoryConfig::from_toml(&s).ok()).and_then(|c| c.repo_share.hub_url).filter(|h| !h.trim().is_empty()))
        .or_else(|| HubCreds::load().map(|c| c.hub))
}

pub async fn login(config: &Path, hub: Option<&str>, provider: &str, no_browser: bool, json_mode: bool) -> Result<()> {
    let hub = default_hub(config, hub).context("sebut hub tim: `engram login --hub https://engram.tim.id` (atau isi [repo_share] hub_url)")?;
    let info = HubClient::anonymous(&hub)?.info().await.with_context(|| format!("hub {hub} tidak bisa dihubungi"))?;
    let provider = provider.trim().to_ascii_lowercase();
    anyhow::ensure!(info.providers.contains(&provider), "hub {} tidak menyediakan login {provider} (tersedia: {})", info.name, info.providers.join(", "));
    let flow = LoginFlow::start(&hub, &provider)?;
    let opened = !no_browser && open_browser(&flow.url);
    if opened {
        eprintln!("Browser dibuka untuk login ke {} lewat {provider}. Menunggu…", info.name);
    } else {
        eprintln!("Buka URL ini di browser untuk login ke {}:\n\n  {}\n\nMenunggu…", info.name, flow.url);
    }
    let creds = flow.wait(Duration::from_secs(300)).await?;
    let v = json!({"hub": creds.hub, "user": creds.user});
    print_out(json_mode, &v, || {
        format!(
            "Login sebagai {} <{}> di {}.\nLangkah berikut di repo proyek: `engram repo init` (repo baru) atau `engram repo join` (repo yang sudah punya .engram/repo.toml).",
            creds.user.name, creds.user.email, creds.hub
        )
    });
    Ok(())
}

pub async fn logout(json_mode: bool) -> Result<()> {
    let Some(c) = HubCreds::load() else {
        print_out(json_mode, &json!({"logged_in": false}), || "Belum login ke hub mana pun.".into());
        return Ok(());
    };
    // Sesi dicabut di hub (best-effort), kredensial lokal selalu dihapus.
    if let Ok(client) = HubClient::anonymous(&c.hub)
        && let Err(e) = client.logout(&c.refresh_token).await
    {
        eprintln!("peringatan: sesi di hub tidak bisa dicabut ({e}); kredensial lokal tetap dihapus");
    }
    HubCreds::clear()?;
    print_out(json_mode, &json!({"logged_out": true, "hub": c.hub}), || format!("Keluar dari {}.", c.hub));
    Ok(())
}

pub async fn whoami(json_mode: bool) -> Result<()> {
    let Ok(client) = HubClient::from_env_or_creds(None) else {
        print_out(json_mode, &json!({"logged_in": false}), || "Belum login. Jalankan `engram login --hub <url>`.".into());
        return Ok(());
    };
    let me = client.me().await?;
    let v = json!({"logged_in": true, "hub": client.base(), "me": me});
    print_out(json_mode, &v, || {
        let mut out = vec![format!("{} <{}> di {}", me.user.name, me.user.email, client.base())];
        out.push(format!("Login lewat: {}", me.identities.iter().map(|i| i.provider.as_str()).collect::<Vec<_>>().join(", ")));
        if me.repos.is_empty() {
            out.push("Belum menjadi anggota repo bersama.".into());
        }
        for r in &me.repos {
            out.push(format!("  - {} (proyek {}) peran {} [{}]", r.name, r.project, r.role, r.id));
        }
        out.join("\n")
    });
    Ok(())
}

pub async fn health(url: &str) -> Result<()> {
    let r = reqwest::Client::builder().timeout(Duration::from_secs(3)).build()?.get(format!("{}/api/health", url.trim_end_matches('/'))).send().await?;
    anyhow::ensure!(r.status().is_success(), "server menjawab {}", r.status());
    println!("ok");
    Ok(())
}
