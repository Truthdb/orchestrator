use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone)]
pub struct GitHub {
    owner: String,
    token: String,
    client: Client,
}

impl GitHub {
    pub fn new(owner: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent("truthdb-orchestrator")
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            owner: owner.into(),
            token: token.into(),
            client,
        })
    }

    pub fn get_release_by_tag(&self, repo: &str, tag: &str) -> Result<Option<Release>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            self.owner, repo, tag
        );

        let resp = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .context("GitHub API request failed")?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            bail!(
                "GitHub API auth failed (status {}). Set GITHUB_TOKEN/GH_TOKEN with access to {}/{}.",
                resp.status(),
                self.owner,
                repo
            );
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("GitHub API error ({}): {}", status, body));
        }

        let release = resp.json::<Release>().context("failed to parse GitHub release JSON")?;
        Ok(Some(release))
    }

    pub fn wait_for_release_assets(
        &self,
        repo: &str,
        tag: &str,
        expected_assets: &[String],
        poll_interval: Duration,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut last_sizes: Option<BTreeMap<String, u64>> = None;
        let mut stable_count = 0u32;

        loop {
            if Instant::now() > deadline {
                bail!(
                    "Timed out waiting for {}/{} {tag} assets: {:?}",
                    self.owner,
                    repo,
                    expected_assets
                );
            }

            let Some(release) = self.get_release_by_tag(repo, tag)? else {
                eprintln!("[{repo}] release {tag} not found yet; waiting...");
                std::thread::sleep(poll_interval);
                continue;
            };

            let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
            for asset in &release.assets {
                sizes.insert(asset.name.clone(), asset.size);
            }

            let mut missing = Vec::new();
            for expected in expected_assets {
                match sizes.get(expected) {
                    Some(sz) if *sz > 0 => {}
                    _ => missing.push(expected.clone()),
                }
            }

            if !missing.is_empty() {
                eprintln!(
                    "[{repo}] waiting for assets (missing {}): {:?}",
                    missing.len(),
                    missing
                );
                std::thread::sleep(poll_interval);
                continue;
            }

            // All assets exist and are non-zero. Now ensure they have stabilized.
            if last_sizes.as_ref() == Some(&sizes) {
                stable_count += 1;
            } else {
                stable_count = 0;
                last_sizes = Some(sizes);
            }

            if stable_count >= 1 {
                eprintln!("[{repo}] assets ready for {tag}");
                return Ok(());
            }

            eprintln!("[{repo}] assets present; verifying stability...");
            std::thread::sleep(poll_interval);
        }
    }
}
