//! GitHub Gist transport for the library sync planner.
//!
//! One Gist is one library: a manifest plus one flat `doc_<uid>.txt` file per
//! document. The API rejects file names containing `/`, which is why the
//! hierarchy lives in the manifest instead of in the Gist file names.

use cove_http::{Client, ClientConfig, Request};
use ruwriter_core::sync::{
    MANIFEST_FILE, Manifest, SyncBase, SyncDocument, SyncReport, base_for, content_hash, gist_name,
    plan_sync,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const API: &str = "https://api.github.com";
const BODY_LIMIT: usize = 32 * 1024 * 1024;
const MAX_FILES: usize = 300;
const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;
const DESCRIPTION_SUFFIX: &str = "（私有云同步，请勿手动编辑）";

#[derive(Clone, Debug)]
pub struct SyncRequest {
    pub gist_id: Option<String>,
    pub library_id: String,
    pub device: String,
    pub local: Vec<SyncDocument>,
    pub base: BTreeMap<String, SyncBase>,
}

#[derive(Clone, Debug)]
struct Remote {
    id: String,
    manifest: Manifest,
    documents: Vec<SyncDocument>,
    files: BTreeSet<String>,
}

/// Finds the library's Gist automatically: a configured id wins, a stale id
/// falls back to the one carrying our library description.
fn resolve_remote(
    client: &Client,
    token: &str,
    request: &SyncRequest,
) -> Result<Option<Remote>, String> {
    let configured = request
        .gist_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    if let Some(id) = configured {
        if let Some(remote) = fetch(client, token, id)? {
            return Ok(Some(remote));
        }
    }
    if let Some(id) = find_gist(client, token, &request.library_id)? {
        return fetch(client, token, &id)?
            .map(Some)
            .ok_or_else(|| "云端 Gist 在查找后消失，请重试同步。".to_owned());
    }
    if configured.is_some() {
        return Err(
            "当前 GitHub 账号无法访问这个资料库的私有 Gist。请确认登录的是原同步账号。".to_owned(),
        );
    }
    Ok(None)
}

fn find_gist(client: &Client, token: &str, library_id: &str) -> Result<Option<String>, String> {
    let marker = format!("Ruwriter 资料库 · {library_id}");
    let description = format!("{marker}{DESCRIPTION_SUFFIX}");
    for page in 1.. {
        let response = client
            .send(auth(
                Request::get(format!("{API}/gists?per_page=100&page={page}")),
                token,
            ))
            .map_err(network_error)?;
        let has_next = response.header("link").is_some_and(|link| {
            link.split(',')
                .any(|entry| entry.trim_end().ends_with("rel=\"next\""))
        });
        let (status, bytes) = read(response)?;
        if status != 200 {
            return Err(api_error(status, &bytes));
        }
        let value = parse_json(&bytes)?;
        let gists = value
            .as_array()
            .ok_or_else(|| "GitHub 返回了无效的 Gist 列表。".to_owned())?;
        for gist in gists {
            let name = gist.get("description").and_then(Value::as_str);
            if gist.get("public").and_then(Value::as_bool) == Some(false)
                && (name == Some(&description) || name == Some(&marker))
                && let Some(id) = gist.get("id").and_then(Value::as_str)
            {
                return Ok(Some(id.to_owned()));
            }
        }
        if !has_next {
            break;
        }
    }
    Ok(None)
}

pub fn sync_with_gist(request: &SyncRequest, token: &str) -> Result<SyncReport, String> {
    prepare_sync(request, token)?.finish(token)
}

pub(crate) struct PendingSync {
    pub report: SyncReport,
    files: Map<String, Value>,
}

fn sync_client() -> Client {
    Client::new(ClientConfig {
        timeout: Duration::from_secs(40),
        max_redirects: 3,
        user_agent: concat!("Ruwriter/", env!("CARGO_PKG_VERSION"), " GistSync").to_owned(),
    })
}

pub(crate) fn prepare_sync(request: &SyncRequest, token: &str) -> Result<PendingSync, String> {
    if token.trim().is_empty() {
        return Err("GitHub CLI 没有返回登录凭据。".to_owned());
    }
    let remote = resolve_remote(&sync_client(), token, request)?;
    if let Some(remote) = &remote
        && remote.manifest.library_id != request.library_id
    {
        return Err("这个 Gist 属于另一个 Ruwriter 资料库。".to_owned());
    }

    let empty_base = BTreeMap::new();
    let base = if remote.is_some() {
        &request.base
    } else {
        &empty_base
    };
    let plan = plan_sync(
        &request.local,
        remote
            .as_ref()
            .map_or(&[], |remote| remote.documents.as_slice()),
        base,
        &request.library_id,
        &request.device,
        ruwriter_core::sync::now(),
    );

    let files = changed_files(remote.as_ref(), &plan)?;
    let created = remote.is_none();
    Ok(PendingSync {
        files,
        report: SyncReport {
            gist_id: remote.map(|remote| remote.id).unwrap_or_default(),
            created,
            base: base_for(&plan.manifest),
            manifest: plan.manifest,
            pulled: plan.downloads,
            deleted: plan.local_deletes,
            conflicts: plan.conflicts,
            summary: if created {
                format!("已创建私有 Gist，{}", plan.summary)
            } else {
                plan.summary
            },
        },
    })
}

fn changed_files(
    remote: Option<&Remote>,
    plan: &ruwriter_core::sync::SyncPlan,
) -> Result<Map<String, Value>, String> {
    let mut files = Map::new();
    for doc in &plan.uploads {
        let name = doc.gist_name();
        if doc.content.is_empty() {
            // A blank document lives only in the manifest.
            if remote.is_some_and(|remote| remote.files.contains(&name)) {
                files.insert(name, Value::Null);
            }
        } else if !remote.is_some_and(|remote| {
            remote.files.contains(&name)
                && remote
                    .documents
                    .iter()
                    .any(|old| old.uid == doc.uid && old.hash == doc.hash)
        }) {
            files.insert(name, json!({ "content": doc.content }));
        }
    }
    if let Some(remote) = remote {
        for doc in &plan.manifest.documents {
            if doc.deleted && remote.files.contains(&doc.file) {
                files.insert(doc.file.clone(), Value::Null);
            }
        }
    }
    if !files.is_empty()
        || remote.is_none_or(|remote| remote.manifest.documents != plan.manifest.documents)
    {
        let manifest_json = serde_json::to_string_pretty(&plan.manifest)
            .map_err(|error| format!("无法生成同步清单：{error}"))?;
        files.insert(
            MANIFEST_FILE.to_owned(),
            json!({ "content": manifest_json }),
        );
    }
    let mut file_count = remote.map_or(0, |remote| remote.files.len());
    for (name, entry) in &files {
        if entry.is_null() {
            file_count -= 1;
        } else {
            if !remote.is_some_and(|remote| remote.files.contains(name)) {
                file_count += 1;
            }
            if entry["content"]
                .as_str()
                .is_some_and(|text| text.len() > MAX_FILE_SIZE)
            {
                return Err(format!(
                    "同步文件「{name}」超过 Gist 可完整读取的 10 MB 上限。"
                ));
            }
        }
    }
    if file_count > MAX_FILES {
        return Err("资料库超过单个 Gist 可完整读取的 300 个文件上限。".to_owned());
    }
    Ok(files)
}

impl PendingSync {
    pub(crate) fn finish(mut self, token: &str) -> Result<SyncReport, String> {
        if self.files.is_empty() {
            return Ok(self.report);
        }
        let client = sync_client();
        if self.report.created {
            let body = json!({
                "description": format!(
                    "Ruwriter 资料库 · {}{DESCRIPTION_SUFFIX}",
                    self.report.manifest.library_id
                ),
                "public": false,
                "files": self.files,
            });
            let response = client
                .send(auth(
                    Request::post(format!("{API}/gists"), body.to_string())
                        .header("Content-Type", "application/json"),
                    token,
                ))
                .map_err(network_error)?;
            let (status, bytes) = read(response)?;
            if status != 201 {
                return Err(api_error(status, &bytes));
            }
            let value = parse_json(&bytes)?;
            self.report.gist_id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "GitHub 没有返回 Gist ID。".to_owned())?
                .to_owned();
        } else {
            patch(&client, token, &self.report.gist_id, self.files)?;
        }
        Ok(self.report)
    }
}

fn fetch(client: &Client, token: &str, id: &str) -> Result<Option<Remote>, String> {
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err("保存的 Gist 标识无效。".to_owned());
    }
    let response = client
        .send(auth(Request::get(format!("{API}/gists/{id}")), token))
        .map_err(network_error)?;
    let (status, bytes) = read(response)?;
    if status == 404 {
        return Ok(None);
    }
    if status != 200 {
        return Err(api_error(status, &bytes));
    }
    let value = parse_json(&bytes)?;
    if value.get("public").and_then(Value::as_bool) != Some(false) {
        return Err("这个 Gist 未设为私有，无法用于云同步。".to_owned());
    }
    if value.get("truncated").and_then(Value::as_bool) == Some(true) {
        return Err("GitHub 只返回了部分 Gist 文件，无法安全地同步这份资料库。".to_owned());
    }
    let files = value
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(|| "GitHub 返回的 Gist 没有文件。".to_owned())?;

    let manifest_info = files
        .get(MANIFEST_FILE)
        .ok_or_else(|| "这个 Gist 不是 Ruwriter 同步库：缺少同步清单。".to_owned())?;
    let manifest_text = fetch_content(client, MANIFEST_FILE, manifest_info)?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("同步清单损坏，未做任何修改：{e}"))?;
    if manifest.library_id.is_empty() {
        return Err("同步清单缺少资料库标识，未做任何修改。".to_owned());
    }
    if manifest.schema > ruwriter_core::sync::SCHEMA {
        return Err("这个 Gist 由更新版本的 Ruwriter 创建，请先升级再同步。".to_owned());
    }

    let mut documents = Vec::new();
    let mut uids = BTreeSet::new();
    for doc in &manifest.documents {
        if doc.uid.is_empty() || !uids.insert(&doc.uid) {
            return Err("同步清单包含无效或重复的文稿标识。".to_owned());
        }
        if doc.deleted {
            documents.push(SyncDocument {
                uid: doc.uid.clone(),
                path: doc.path.clone(),
                content: String::new(),
                hash: String::new(),
                updated_at: doc.updated_at,
                deleted: true,
                title: doc.title.clone(),
                synopsis: doc.synopsis.clone(),
                status: doc.status,
                tags: doc.tags.clone(),
                pinned: doc.pinned,
                created_at: doc.created_at,
            });
            continue;
        }
        let file = if doc.file.is_empty() {
            gist_name(&doc.uid)
        } else {
            doc.file.clone()
        };
        let content = match files.get(&file) {
            Some(info) => fetch_content(client, &file, info)?,
            // Empty documents deliberately have no Gist file (the API rejects
            // empty content). The manifest hash tells us this is intentional.
            None if doc.hash == content_hash("") => String::new(),
            None => {
                return Err(format!(
                    "同步清单里的《{}》缺少正文文件 {}，未做任何修改。",
                    doc.title, file
                ));
            }
        };
        documents.push(SyncDocument {
            uid: doc.uid.clone(),
            path: doc.path.clone(),
            hash: content_hash(&content),
            content,
            updated_at: doc.updated_at,
            deleted: false,
            title: doc.title.clone(),
            synopsis: doc.synopsis.clone(),
            status: doc.status,
            tags: doc.tags.clone(),
            pinned: doc.pinned,
            created_at: doc.created_at,
        });
    }
    Ok(Some(Remote {
        id: id.to_owned(),
        manifest,
        documents,
        files: files.keys().cloned().collect(),
    }))
}

fn fetch_content(client: &Client, name: &str, info: &Value) -> Result<String, String> {
    if info.get("truncated").and_then(Value::as_bool) != Some(true)
        && let Some(content) = info.get("content").and_then(Value::as_str)
    {
        return Ok(content.to_owned());
    }
    let raw = info
        .get("raw_url")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("GitHub 没有返回「{name}」的完整内容。"))?;
    fetch_raw(client, raw)
}

fn fetch_raw(client: &Client, url: &str) -> Result<String, String> {
    let response = client
        .send(Request::get(url).header("Accept", "text/plain"))
        .map_err(network_error)?;
    let (status, bytes) = read(response)?;
    if status != 200 {
        return Err(format!("读取 Gist 原始文稿失败（HTTP {status}）。"));
    }
    String::from_utf8(bytes).map_err(|_| "Gist 文稿不是有效的 UTF-8 文本。".to_owned())
}

/// Deletes a Gist; used by the live sync test and by future cleanup UI.
pub fn delete_gist(token: &str, id: &str) -> Result<(), String> {
    let client = Client::new(ClientConfig {
        timeout: Duration::from_secs(20),
        max_redirects: 2,
        user_agent: "Ruwriter/0.3 GistSync".to_owned(),
    });
    let response = client
        .send(auth(
            Request::new("DELETE", format!("{API}/gists/{id}")),
            token,
        ))
        .map_err(network_error)?;
    let (status, bytes) = read(response)?;
    if status == 204 {
        Ok(())
    } else {
        Err(api_error(status, &bytes))
    }
}

fn patch(client: &Client, token: &str, id: &str, files: Map<String, Value>) -> Result<(), String> {
    let body = json!({ "files": files });
    let response = client
        .send(auth(
            Request::new("PATCH", format!("{API}/gists/{id}"))
                .header("Content-Type", "application/json")
                .body(body.to_string()),
            token,
        ))
        .map_err(network_error)?;
    if response.status == 200 {
        return Ok(());
    }
    let (status, bytes) = read(response)?;
    Err(api_error(status, &bytes))
}

fn auth(request: Request, token: &str) -> Request {
    request
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
}

fn read(response: cove_http::Response) -> Result<(u16, Vec<u8>), String> {
    let status = response.status;
    let bytes = response
        .bytes(BODY_LIMIT)
        .map_err(|e| format!("读取 GitHub 响应失败：{e}"))?;
    Ok((status, bytes))
}

fn parse_json(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|_| "GitHub 返回了无法解析的数据。".to_owned())
}

fn api_error(status: u16, bytes: &[u8]) -> String {
    let message = parse_json(bytes)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| "未知错误".to_owned());
    match status {
        401 => format!("GitHub Token 无效或已过期（401：{message}）。"),
        403 => format!("GitHub 拒绝了这个请求（403：{message}）。请确认 Token 有 gist 权限。"),
        404 => format!("找不到 Gist（404：{message}）。"),
        422 => format!("GitHub 无法接受同步内容（422：{message}）。"),
        _ => format!("GitHub 返回 HTTP {status}：{message}"),
    }
}

fn network_error(error: cove_http::HttpError) -> String {
    format!("连接 GitHub 失败：{error}")
}

/// Runs a `gh` command without showing a console window on Windows.
fn gh_command(args: &[&str]) -> Result<std::process::Output, String> {
    let run = |path: &std::path::Path| {
        let mut command = std::process::Command::new(path);
        command.args(args).env("NO_COLOR", "1");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        command.output()
    };
    let result = run(std::path::Path::new("gh"));
    // Finder-launched apps commonly inherit only the system PATH.
    #[cfg(target_os = "macos")]
    let result = {
        let mut result = result;
        for path in [
            "/opt/homebrew/bin/gh",
            "/usr/local/bin/gh",
            "/opt/local/bin/gh",
        ] {
            if !result
                .as_ref()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            {
                break;
            }
            result = run(std::path::Path::new(path));
        }
        result
    };
    result.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "没有找到 GitHub CLI。请先安装 gh，再在终端运行 gh auth login。Ruwriter 不保存 Token。"
                .to_owned()
        } else {
            format!("无法启动 GitHub CLI：{error}")
        }
    })
}

/// Reads the existing GitHub CLI login for one sync round. Nothing is written
/// to disk; the token lives only in memory for the duration of the request.
pub fn gh_token() -> Result<String, String> {
    let output = gh_command(&["auth", "token", "--hostname", "github.com"])?;
    if !output.status.success() {
        return Err(
            "GitHub CLI 还没有登录。请在终端运行 gh auth login，完成后回到这里点“立即同步”。"
                .to_owned(),
        );
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if token.is_empty() {
        return Err("GitHub CLI 没有返回登录凭据。请运行 gh auth login 后重试。".to_owned());
    }
    Ok(token)
}

/// Login status for the dialog, parsed from `gh auth status`; never returns a
/// token.
pub fn gh_status() -> String {
    let output = match gh_command(&["auth", "status", "--hostname", "github.com", "--active"]) {
        Ok(output) => output,
        Err(error) => return error,
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return "GitHub CLI 未登录：在终端运行 gh auth login 后回来点“检查登录”。".to_owned();
    }
    for line in text.lines() {
        if let Some(rest) = line.split("Logged in to github.com account ").nth(1) {
            let account = rest.split_whitespace().next().unwrap_or(rest).trim();
            return format!("GitHub CLI 已登录：{account}");
        }
    }
    "GitHub CLI 已登录".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruwriter_core::Session;

    #[test]
    fn blank_documents_update_the_manifest_without_deleting_absent_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::at(dir.path());
        let local = session.sync_snapshot().unwrap();
        let blank = &local[0];
        let name = blank.gist_name();
        let plan = plan_sync(&local, &[], &BTreeMap::new(), "lib", "a", 100);
        let created = changed_files(None, &plan).unwrap();
        assert_eq!(created.len(), 1);
        assert!(created.contains_key(MANIFEST_FILE));

        let mut remote = Remote {
            id: "test".to_owned(),
            manifest: plan.manifest,
            documents: local.clone(),
            files: BTreeSet::from([MANIFEST_FILE.to_owned()]),
        };
        let unchanged = plan_sync(
            &local,
            &remote.documents,
            &base_for(&remote.manifest),
            "lib",
            "b",
            101,
        );
        assert!(changed_files(Some(&remote), &unchanged).unwrap().is_empty());

        remote.files.insert(name.clone());
        remote.documents[0].content = "雨夜".to_owned();
        remote.documents[0].hash = content_hash("雨夜");
        let mut previous = remote.manifest.clone();
        previous.documents[0].hash = remote.documents[0].hash.clone();
        remote.manifest = previous;
        let emptied = plan_sync(
            &local,
            &remote.documents,
            &base_for(&remote.manifest),
            "lib",
            "b",
            102,
        );
        assert_eq!(
            changed_files(Some(&remote), &emptied).unwrap().get(&name),
            Some(&Value::Null)
        );
    }
}
