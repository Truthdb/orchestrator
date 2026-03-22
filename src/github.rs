use anyhow::{Context, Result, anyhow, bail};
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::reporter::Reporter;

pub const PRIMARY_GITHUB_TOKEN_ENV: &str = "GITHUB_TRUTHDB_TOKEN";
pub const FALLBACK_GITHUB_TOKEN_ENV: &str = "GH_TOKEN";
pub const LEGACY_GITHUB_TOKEN_ENV: &str = "GITHUB_TOKEN";

pub fn github_token() -> String {
    std::env::var(PRIMARY_GITHUB_TOKEN_ENV)
        .or_else(|_| std::env::var(FALLBACK_GITHUB_TOKEN_ENV))
        .or_else(|_| std::env::var(LEGACY_GITHUB_TOKEN_ENV))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfo {
    pub default_branch: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRunsResponse {
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRun {
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LatestRelease {
    pub tag_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompareResponse {
    pub ahead_by: u32,
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

    fn get(&self, url: String) -> reqwest::blocking::RequestBuilder {
        let req = self.client.get(url);
        if self.token.trim().is_empty() {
            req
        } else {
            req.bearer_auth(&self.token)
        }
    }

    fn send_get(&self, url: &str) -> Result<Response> {
        let resp = self
            .get(url.to_string())
            .send()
            .context("GitHub API request failed")?;

        if !self.token.trim().is_empty()
            && matches!(
                resp.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            )
        {
            let retry = self
                .client
                .get(url)
                .send()
                .context("GitHub API request failed")?;
            return Ok(retry);
        }

        Ok(resp)
    }

    pub fn get_release_by_tag(&self, repo: &str, tag: &str) -> Result<Option<Release>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            self.owner, repo, tag
        );

        let resp = self.send_get(&url)?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            bail!(
                "GitHub API auth failed (status {}). Set {}, {}, or {} with access to {}/{}.",
                resp.status(),
                PRIMARY_GITHUB_TOKEN_ENV,
                FALLBACK_GITHUB_TOKEN_ENV,
                LEGACY_GITHUB_TOKEN_ENV,
                self.owner,
                repo
            );
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("GitHub API error ({}): {}", status, body));
        }

        let release = resp
            .json::<Release>()
            .context("failed to parse GitHub release JSON")?;
        Ok(Some(release))
    }

    pub fn get_default_branch(&self, repo: &str) -> Result<String> {
        let url = format!("https://api.github.com/repos/{}/{repo}", self.owner);
        let resp = self.send_get(&url)?;

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            bail!(
                "GitHub API auth failed (status {}). Set {}, {}, or {} with access to {}/{}.",
                resp.status(),
                PRIMARY_GITHUB_TOKEN_ENV,
                FALLBACK_GITHUB_TOKEN_ENV,
                LEGACY_GITHUB_TOKEN_ENV,
                self.owner,
                repo
            );
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("GitHub API error ({}): {}", status, body));
        }

        let info = resp
            .json::<RepoInfo>()
            .context("failed to parse GitHub repo JSON")?;
        Ok(info.default_branch)
    }

    pub fn get_latest_workflow_run(&self, repo: &str) -> Result<Option<WorkflowRun>> {
        let url = format!(
            "https://api.github.com/repos/{}/{repo}/actions/runs?per_page=1",
            self.owner
        );

        let resp = self.send_get(&url)?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            bail!(
                "GitHub API auth failed (status {}). Set {}, {}, or {} with access to {}/{}.",
                resp.status(),
                PRIMARY_GITHUB_TOKEN_ENV,
                FALLBACK_GITHUB_TOKEN_ENV,
                LEGACY_GITHUB_TOKEN_ENV,
                self.owner,
                repo
            );
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("GitHub API error ({}): {}", status, body));
        }

        let data = resp
            .json::<WorkflowRunsResponse>()
            .context("failed to parse GitHub workflow runs JSON")?;
        Ok(data.workflow_runs.into_iter().next())
    }

    pub fn get_latest_release_tag(&self, repo: &str) -> Result<Option<String>> {
        let url = format!(
            "https://api.github.com/repos/{}/{repo}/releases/latest",
            self.owner
        );

        let resp = self.send_get(&url)?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            bail!(
                "GitHub API auth failed (status {}). Set {}, {}, or {} with access to {}/{}.",
                resp.status(),
                PRIMARY_GITHUB_TOKEN_ENV,
                FALLBACK_GITHUB_TOKEN_ENV,
                LEGACY_GITHUB_TOKEN_ENV,
                self.owner,
                repo
            );
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("GitHub API error ({}): {}", status, body));
        }

        let release = resp
            .json::<LatestRelease>()
            .context("failed to parse GitHub latest release JSON")?;
        Ok(Some(release.tag_name))
    }

    pub fn compare_ahead_by(&self, repo: &str, base: &str, head: &str) -> Result<u32> {
        let url = format!(
            "https://api.github.com/repos/{}/{repo}/compare/{}...{}",
            self.owner, base, head
        );

        let resp = self.send_get(&url)?;

        if resp.status() == StatusCode::NOT_FOUND {
            bail!("compare not available for {}/{}", self.owner, repo);
        }

        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            bail!(
                "GitHub API auth failed (status {}). Set {}, {}, or {} with access to {}/{}.",
                resp.status(),
                PRIMARY_GITHUB_TOKEN_ENV,
                FALLBACK_GITHUB_TOKEN_ENV,
                LEGACY_GITHUB_TOKEN_ENV,
                self.owner,
                repo
            );
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(anyhow!("GitHub API error ({}): {}", status, body));
        }

        let cmp = resp
            .json::<CompareResponse>()
            .context("failed to parse GitHub compare JSON")?;
        Ok(cmp.ahead_by)
    }

    pub fn wait_for_release_assets(
        &self,
        repo: &str,
        tag: &str,
        expected_assets: &[String],
        poll_interval: Duration,
        timeout: Duration,
        reporter: &dyn Reporter,
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
                reporter.update(format!("[{repo}] release {tag} not found yet; waiting…"));
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
                reporter.update(format!(
                    "[{repo}] waiting for assets (missing {}): {:?}",
                    missing.len(),
                    missing
                ));
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
                reporter.update(format!("[{repo}] assets ready for {tag}"));
                return Ok(());
            }

            reporter.update(format!("[{repo}] assets present; verifying stability…"));
            std::thread::sleep(poll_interval);
        }
    }
}
