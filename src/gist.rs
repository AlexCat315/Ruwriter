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

#[derive(Clone, Debug)]
pub struct SyncRequest {
    pub token: String,
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
}

enum Resolved {
    Existing(Remote, BTreeMap<String, SyncBase>),
    Created(SyncReport),
}

/// Finds the library's Gist automatically: a configured id wins, a stale id
/// falls back to the one carrying our library description, and a fresh library
/// creates one. No user-facing Gist id is required.
fn resolve_remote(client: &Client, token: &str, request: &SyncRequest) -> Result<Resolved, String> {
    if let Some(id) = request
        .gist_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        match fetch(client, token, id.trim()) {
            Ok(remote) => return Ok(Resolved::Existing(remote, request.base.clone())),
            Err(error) if error.starts_with("找不到这个 Gist") => {}
            Err(error) => return Err(error),
        }
    }
    if let Some(id) = find_gist(client, token, &request.library_id)? {
        return Ok(Resolved::Existing(
            fetch(client, token, &id)?,
            BTreeMap::new(),
        ));
    }
    create(request, token, client).map(Resolved::Created)
}

fn find_gist(client: &Client, token: &str, library_id: &str) -> Result<Option<String>, String> {
    let response = client
        .send(auth(
            Request::get(format!("{API}/gists?per_page=100")),
            token,
        ))
        .map_err(network_error)?;
    let (status, bytes) = read(response)?;
    if status != 200 {
        return Err(api_error(status, &bytes));
    }
    let value = parse_json(&bytes)?;
    let Some(gists) = value.as_array() else {
        return Ok(None);
    };
    let marker = format!("Ruwriter 资料库 · {library_id}");
    for gist in gists {
        let description = gist
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !description.contains(&marker) {
            continue;
        }
        if let Some(id) = gist.get("id").and_then(Value::as_str) {
            return Ok(Some(id.to_owned()));
        }
    }
    Ok(None)
}

pub fn sync_with_gist(request: &SyncRequest) -> Result<SyncReport, String> {
    let token = request.token.trim();
    if token.is_empty() {
        return Err("还没有填写 GitHub Token。".to_owned());
    }
    let client = Client::new(ClientConfig {
        timeout: Duration::from_secs(40),
        max_redirects: 3,
        user_agent: "Ruwriter/0.3 GistSync".to_owned(),
    });

    let (remote, base) = match resolve_remote(&client, token, request)? {
        Resolved::Existing(remote, base) => (remote, base),
        Resolved::Created(report) => return Ok(report),
    };
    if remote.manifest.library_id != request.library_id {
        return Err(format!(
            "这个 Gist 属于另一个 Ruwriter 资料库（{}）。",
            remote.manifest.library_id
        ));
    }

    let plan = plan_sync(
        &request.local,
        &remote.documents,
        &base,
        &request.library_id,
        &request.device,
        ruwriter_core::sync::now(),
    );
    let mut files = Map::new();
    for doc in &plan.uploads {
        // Gist rejects an empty `content` ("missing_field: files"). An empty
        // document is represented by its manifest entry alone; fetching it
        // back restores empty text from the recorded hash.
        if doc.content.is_empty() {
            files.insert(doc.gist_name(), Value::Null);
        } else {
            files.insert(doc.gist_name(), json!({ "content": doc.content }));
        }
    }
    let remote_active: BTreeSet<String> = remote
        .documents
        .iter()
        .filter(|d| !d.deleted)
        .map(|d| d.uid.clone())
        .collect();
    for doc in &plan.manifest.documents {
        if doc.deleted && remote_active.contains(&doc.uid) {
            files.insert(doc.file.clone(), Value::Null);
        }
    }
    let manifest_json = serde_json::to_string_pretty(&plan.manifest)
        .map_err(|e| format!("无法生成同步清单：{e}"))?;
    files.insert(
        MANIFEST_FILE.to_owned(),
        json!({ "content": manifest_json }),
    );
    patch(&client, token, &remote.id, files)?;

    let base = base_for(&plan.manifest);
    Ok(SyncReport {
        gist_id: remote.id,
        created: false,
        manifest: plan.manifest,
        pulled: plan.downloads,
        deleted: plan.local_deletes,
        conflicts: plan.conflicts,
        summary: plan.summary,
        base,
    })
}

fn create(request: &SyncRequest, token: &str, client: &Client) -> Result<SyncReport, String> {
    let plan = plan_sync(
        &request.local,
        &[],
        &BTreeMap::new(),
        &request.library_id,
        &request.device,
        ruwriter_core::sync::now(),
    );
    let mut files = Map::new();
    for doc in &plan.uploads {
        if doc.content.is_empty() {
            continue;
        }
        files.insert(doc.gist_name(), json!({ "content": doc.content }));
    }
    let manifest_json = serde_json::to_string_pretty(&plan.manifest)
        .map_err(|e| format!("无法生成同步清单：{e}"))?;
    files.insert(
        MANIFEST_FILE.to_owned(),
        json!({ "content": manifest_json }),
    );
    let body = json!({
        "description": format!(
            "Ruwriter 资料库 · {}（私有云同步，请勿手动编辑）",
            request.library_id
        ),
        "public": false,
        "files": files,
    });
    let response = client
        .send(auth(
            Request::post(format!("{API}/gists"), body.to_string()),
            token,
        ))
        .map_err(network_error)?;
    let (status, bytes) = read(response)?;
    if status != 201 {
        return Err(api_error(status, &bytes));
    }
    let value = parse_json(&bytes)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "GitHub 没有返回 Gist ID。".to_owned())?
        .to_owned();
    let base = base_for(&plan.manifest);
    Ok(SyncReport {
        gist_id: id,
        created: true,
        manifest: plan.manifest,
        pulled: Vec::new(),
        deleted: Vec::new(),
        conflicts: Vec::new(),
        summary: format!(
            "已创建私有 Gist，{}",
            if plan.uploads.is_empty() {
                "资料库已上传".to_owned()
            } else {
                plan.summary.clone()
            }
        ),
        base,
    })
}

fn fetch(client: &Client, token: &str, id: &str) -> Result<Remote, String> {
    let response = client
        .send(auth(Request::get(format!("{API}/gists/{id}")), token))
        .map_err(network_error)?;
    let (status, bytes) = read(response)?;
    if status == 404 {
        return Err(
            "找不到这个 Gist。请检查 Gist ID，或把 Gist ID 留空让 Ruwriter 新建一个。".to_owned(),
        );
    }
    if status != 200 {
        return Err(api_error(status, &bytes));
    }
    let value = parse_json(&bytes)?;
    let files = value
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(|| "GitHub 返回的 Gist 没有文件。".to_owned())?;

    let mut contents = BTreeMap::new();
    for (name, info) in files {
        let truncated = info
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let content = if truncated {
            let raw = info
                .get("raw_url")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{name} 太大，GitHub 没有提供原始下载地址。"))?;
            fetch_raw(client, raw)?
        } else {
            info.get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        contents.insert(name.clone(), content);
    }

    let manifest_text = contents
        .get(MANIFEST_FILE)
        .ok_or_else(|| "这个 Gist 不是 Ruwriter 同步库：缺少同步清单。".to_owned())?;
    let mut manifest: Manifest = serde_json::from_str(manifest_text)
        .map_err(|e| format!("同步清单损坏，未做任何修改：{e}"))?;
    if manifest.library_id.is_empty() {
        return Err("同步清单缺少资料库标识，未做任何修改。".to_owned());
    }
    if manifest.schema > ruwriter_core::sync::SCHEMA {
        return Err("这个 Gist 由更新版本的 Ruwriter 创建，请先升级再同步。".to_owned());
    }

    let mut documents = Vec::new();
    for doc in &manifest.documents {
        if doc.uid.is_empty() {
            continue;
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
        let content = match contents.get(&file) {
            Some(content) => content.clone(),
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
            content: content.clone(),
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
    // Keep the manifest's own metadata for deleted entries; `documents`
    // only contains active ones, which is what the planner needs.
    manifest
        .documents
        .retain(|doc| !doc.deleted || !doc.uid.is_empty());
    Ok(Remote {
        id: id.to_owned(),
        manifest,
        documents,
    })
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
            Request::new("PATCH", format!("{API}/gists/{id}")).body(body.to_string()),
            token,
        ))
        .map_err(network_error)?;
    let (status, bytes) = read(response)?;
    if status != 200 {
        return Err(api_error(status, &bytes));
    }
    Ok(())
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
    let mut command = std::process::Command::new("gh");
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.output().map_err(|error| {
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
    let output = gh_command(&["auth", "token"])?;
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
    let output = match gh_command(&["auth", "status"]) {
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
