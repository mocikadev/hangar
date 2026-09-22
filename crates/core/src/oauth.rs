use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_http::{Response, Server};
use url::Url;

use crate::account::Account;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH_ENDPOINT: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const SCOPES: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";
const ORIGINATOR: &str = "Codex Desktop";
const CALLBACK_PORT: u16 = 1455;
/// 官方 Hydra 白名单仅允许 1455 与 1457（见 openai/codex server.rs FALLBACK_PORT），
/// 回退禁止用随机端口，否则授权/交换被拒
const FALLBACK_CALLBACK_PORT: u16 = 1457;
const HOSTED_AUTH_ENDPOINT: &str = "https://chatgpt.com/codex/desktop-auth";

fn generate_code_verifier() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(digest)
}

fn generate_state_token() -> String {
    let bytes: Vec<u8> = (0..16).map(|_| rand::random::<u8>()).collect();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// 回调服务绑定：直接 try 1455→1457（官方白名单仅此二者），无 probe 避免 TOCTOU。
/// redirect_uri 与官方一致用 localhost + /auth/callback（Hydra 精确匹配）；
/// 服务端仍绑 127.0.0.1（与官方 run_login_server 一致，避开 ::1 解析问题）
fn bind_callback_server() -> Result<(std::sync::Arc<Server>, u16), String> {
    let mut last_err = String::new();
    for port in [CALLBACK_PORT, FALLBACK_CALLBACK_PORT] {
        match Server::http(format!("127.0.0.1:{}", port)) {
            Ok(s) => return Ok((std::sync::Arc::new(s), port)),
            Err(e) => {
                // tiny_http 返回 Box<dyn Error>，无 kind()，按文案识别端口占用
                let msg = e.to_string().to_ascii_lowercase();
                last_err = e.to_string();
                if msg.contains("in use") || msg.contains("addrinuse") {
                    continue;
                }
                return Err(format!("启动本地服务器失败: {}", e));
            }
        }
    }
    Err(format!(
        "端口 {} 与 {} 均被占用（通常是 Codex CLI 正在登录），请稍后重试；最后错误: {}",
        CALLBACK_PORT, FALLBACK_CALLBACK_PORT, last_err
    ))
}

/// 登录交互钩子：默认 stdin 实现供 TUI/classic；GUI 传自己的弹窗实现。
/// `prompt_callback` 返回用户粘贴的原始 URL（None=放弃），解析与校验统一在 core。
pub trait LoginHooks {
    fn show_auth_url(&self, url: &str);
    fn prompt_callback(&self, state: &str) -> Option<String>;
}

/// 终端默认实现：行为与原 `prompt_manual_callback` 逐行一致
pub struct StdinHooks;

impl LoginHooks for StdinHooks {
    fn show_auth_url(&self, url: &str) {
        crate::emit::emit_err(url.to_string());
    }

    fn prompt_callback(&self, _state: &str) -> Option<String> {
        use std::io;
        crate::emit::emit_err("浏览器回调未收到（可能端口/跳转被拦截）。".to_string());
        crate::emit::emit_err(
            "可将浏览器地址栏的完整回调 URL 粘贴到此处（直接回车放弃）：".to_string(),
        );
        crate::emit::emit_err("回调 URL>".to_string());
        let mut line = String::new();
        if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            return None;
        }
        Some(line)
    }
}

/// 超时后手动兜底：经 hooks 取回粘贴的 URL，解析 code 并校验 state
fn prompt_manual_callback_with(hooks: &dyn LoginHooks, expected_state: &str) -> Option<String> {
    let line = hooks.prompt_callback(expected_state)?;
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    match parse_code_from_callback_url(line, expected_state) {
        Ok(code) => Some(code),
        Err(e) => {
            // GUI 调用前 set_quiet(true) 屏蔽此行（TUI 后台线程同款），对话框内用
            // 公开的 parse_code_from_callback_url 预检并展示具体错误
            crate::emit::emit_err(format!("回调解析失败: {}", e));
            None
        }
    }
}

/// 回调 URL 解析（公开供 GUI 预检输入，规则与主流程共用一份）
pub fn parse_code_from_callback_url(url: &str, expected_state: &str) -> Result<String, String> {
    let parsed = Url::parse(url.trim())
        .map_err(|e| format!("URL 格式无效（请粘贴 http:// 开头的完整地址）: {}", e))?;
    // 仅接受本地回调地址，防止粘错页面 URL 蒙混过关
    let host_ok = matches!(parsed.host_str(), Some("localhost") | Some("127.0.0.1"));
    if !host_ok || parsed.path() != "/auth/callback" {
        return Err(
            "回调地址无效：应为 http://localhost:1455（或1457）/auth/callback 开头的浏览器地址栏 URL"
                .to_string(),
        );
    }
    let qs: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
    let state = qs.get("state").map(|v| v.to_string()).unwrap_or_default();
    if state != expected_state {
        return Err("state 不匹配（请确认是本次登录的回调链接）".to_string());
    }
    qs.get("code")
        .map(|v| v.to_string())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "缺少 code 参数".to_string())
}

/// 执行 OAuth 登录流程（阻塞式，终端默认交互）：
/// 打开浏览器 → 监听 1455 回调 → 换取 token → 返回账号
pub fn login_codex() -> Result<Account, String> {
    login_codex_with(&StdinHooks)
}

/// 执行 OAuth 登录流程（阻塞式，交互经 hooks 注入）：
/// 打开浏览器 → 监听 1455 回调 → 换取 token → 返回账号
pub fn login_codex_with(hooks: &dyn LoginHooks) -> Result<Account, String> {
    // 1. 生成 PKCE 参数
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);
    let state = generate_state_token();

    // 2. 启动本地回调服务器：直连 1455→1457（官方白名单仅此二者，无 probe 无竞态）
    // redirect_uri 与官方一致用 localhost + /auth/callback；
    // Arc 持有：超时后主线程需要 unblock() 唤醒阻塞在 incoming_requests()
    // 的回调线程，否则线程泄漏、端口被占，重试登录必然失败
    let (server, callback_port) = bind_callback_server()?;
    if callback_port != CALLBACK_PORT {
        crate::emit::emit_err(format!(
            "端口 {} 被占用，已改用官方备用端口 {} 继续登录",
            CALLBACK_PORT, callback_port
        ));
    }
    let redirect_uri = format!("http://localhost:{}/auth/callback", callback_port);

    let auth_received = Arc::new(AtomicBool::new(false));
    let auth_received_clone = auth_received.clone();
    let code_received = Arc::new(std::sync::Mutex::new(None::<String>));
    let code_received_clone = code_received.clone();
    let state_expected = state.clone();

    // 3. 构建授权 URL（PKCE + 官方客户端参数）
    let mut auth_url = Url::parse(AUTH_ENDPOINT).expect("valid authorize endpoint");
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", CLIENT_ID);
        q.append_pair("redirect_uri", &redirect_uri);
        q.append_pair("scope", SCOPES);
        q.append_pair("code_challenge", &code_challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("id_token_add_organizations", "true");
        q.append_pair("codex_cli_simplified_flow", "true");
        q.append_pair("codex_streamlined_login", "true");
        q.append_pair("state", &state);
        q.append_pair("originator", ORIGINATOR);
    }

    // 包装成官方桌面使用的 hosted login 地址
    let final_url = format!(
        "{}?authorize_url={}&codex_streamlined_login=true&no_universal_links=1",
        HOSTED_AUTH_ENDPOINT,
        urlencode(auth_url.as_str()),
    );

    // 4. 打开浏览器（无头机无浏览器时打印 URL 手动复制，不阻断）
    if open::that(&final_url).is_err() {
        crate::emit::emit_err("无法自动打开浏览器，请手动访问以下地址完成授权：".to_string());
    } else {
        crate::emit::emit_err("已打开浏览器进行授权，请在 5 分钟内完成登录...".to_string());
    }
    hooks.show_auth_url(&final_url);

    // 5. 等待回调
    // 线程持有一份 Arc，主线程保留原份用于超时后 unblock()。
    // auth_error：用户拒绝授权时线程早退，主循环立即报错而非空等 5 分钟
    let auth_error = Arc::new(std::sync::Mutex::new(None::<String>));
    let auth_error_clone = auth_error.clone();
    let server_for_thread = server.clone();
    let handle = std::thread::spawn(move || {
        for request in server_for_thread.incoming_requests() {
            let url_str = format!("http://localhost{}", request.url());
            let url = match Url::parse(&url_str) {
                Ok(u) => u,
                Err(_) => {
                    let resp = Response::from_string("请求错误").with_status_code(400);
                    let _ = request.respond(resp);
                    continue;
                }
            };

            if url.path() == "/auth/callback" {
                let qs: std::collections::HashMap<_, _> = url.query_pairs().collect();
                if let Some(code) = qs.get("code").map(|v| v.to_string()) {
                    // CSRF 防护：回调 state 必须与授权请求发出的一致，
                    // 不匹配视为伪造/串会话回调，拒绝该次但继续监听（不 return，
                    // 否则一次误触即杀死监听，主循环空等到超时）
                    let returned = qs.get("state").map(|v| v.to_string()).unwrap_or_default();
                    if returned != state_expected {
                        let resp = Response::from_string("state 校验失败，已拒绝该回调")
                            .with_status_code(400);
                        let _ = request.respond(resp);
                        continue;
                    }
                    // 锁投毒容错：回调线程 panic 不应直接 abort（release panic=abort 会跳过锁文件 Drop
                    // 致残留锁）；取内部值继续，保证主循环能正常超时/报错退出
                    *code_received_clone
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(code);
                    auth_received_clone.store(true, Ordering::SeqCst);
                    let resp = Response::from_string("授权成功！请关闭此窗口返回终端。")
                        .with_status_code(200);
                    let _ = request.respond(resp);
                    return;
                }
                // 用户拒绝/IdP 报错：记录后早退，主循环立即返回而非等满 5 分钟
                let error = qs.get("error").map(|v| v.to_string()).unwrap_or_default();
                let desc = qs
                    .get("error_description")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let msg = if desc.is_empty() {
                    format!("授权失败: {}", error)
                } else {
                    format!("授权失败: {}（{}）", error, desc)
                };
                *auth_error_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
                let resp = Response::from_string(msg).with_status_code(400);
                let _ = request.respond(resp);
                return;
            }

            let resp = Response::from_string("Not Found").with_status_code(404);
            let _ = request.respond(resp);
        }
    });

    // 6. 等待授权（超时 5 分钟，拒绝则早退；超时后支持手动粘贴回调 URL 兜底）
    let mut denied: Option<String> = None;
    for _ in 0..300 {
        if auth_received.load(Ordering::SeqCst) {
            break;
        }
        if let Some(e) = auth_error.lock().unwrap_or_else(|e| e.into_inner()).take() {
            denied = Some(e);
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    // 拒绝授权：唤醒线程释放端口后直接返回
    if let Some(e) = denied {
        server.unblock();
        let _ = handle.join();
        return Err(e);
    }

    if !auth_received.load(Ordering::SeqCst) {
        // 唤醒阻塞在 incoming_requests() 的回调线程，先释放端口再提示手动粘贴
        server.unblock();
        let _ = handle.join();
        if let Some(code) = prompt_manual_callback_with(hooks, &state) {
            *code_received.lock().unwrap_or_else(|e| e.into_inner()) = Some(code);
            auth_received.store(true, Ordering::SeqCst);
        } else {
            return Err("授权超时，请在 5 分钟内完成浏览器中的操作".to_string());
        }
    } else {
        let _ = handle.join();
    }

    let code = code_received
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| "未获取到授权码".to_string())?;

    // 7. 换取 token
    crate::emit::emit_err("授权成功，正在获取 token...".to_string());
    let token = exchange_code_for_token(&code, &redirect_uri, &code_verifier)?;

    // 8. 获取用户邮箱
    // 身份来源优先级：id_token（OIDC JWT，openid scope，零网络）→ userinfo 兜底。
    // userinfo 失败时保留根因一并提示，不吞错
    let email =
        match crate::account::email_from_id_token(&token.id_token.clone().unwrap_or_default()) {
            Some(e) => e,
            None => get_user_email(&token.access_token).map_err(|e| {
                format!(
                    "无法确定账号邮箱（id_token 无 email 且 userinfo 失败: {}）",
                    e
                )
            })?,
        };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // expires_in 缺失时回退 1 小时（官方 AT 小时级寿命），避免 expires_at=0
    // 导致每次切换都烧一次 RT 轮换；saturating_add 防服务端异常大值溢出 panic
    let expires_at = crate::token_health::jwt_exp(&token.access_token)
        .and_then(|exp| u64::try_from(exp).ok())
        .or_else(|| token.expires_in.map(|exp| now.saturating_add(exp)))
        .unwrap_or_else(|| now.saturating_add(3600));

    Ok(Account {
        id: uuid::Uuid::new_v4().to_string(),
        email,
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.unwrap_or_default(),
        id_token: token.id_token.unwrap_or_default(),
        expires_at,
        stale: false,
        account_id: crate::account::extract_chatgpt_account_id(&token.access_token),
        organization_id: crate::account::extract_chatgpt_org_id(&token.access_token),
    })
}

#[derive(serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_in: Option<u64>,
    #[allow(dead_code)]
    pub token_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshError {
    AuthRejected {
        status: u16,
        code: Option<String>,
        body_len: usize,
    },
    Transport(String),
    InvalidResponse(String),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthRejected {
                status,
                code,
                body_len,
            } => {
                write!(f, "Token 刷新请求失败: HTTP {status}")?;
                if let Some(code) = code {
                    write!(f, ", error_code={code}")?;
                }
                write!(f, ", body_len={body_len}")
            }
            Self::Transport(message) | Self::InvalidResponse(message) => f.write_str(message),
        }
    }
}

impl RefreshError {
    pub fn is_auth_rejection(&self) -> bool {
        match self {
            Self::AuthRejected { status, code, .. } => {
                *status == 401
                    || code.as_deref().is_some_and(|code| {
                        matches!(
                            code.to_ascii_lowercase().as_str(),
                            "invalid_grant"
                                | "refresh_token_reused"
                                | "token_invalidated"
                                | "authentication_token_invalidated"
                                | "unauthorized"
                        )
                    })
            }
            _ => false,
        }
    }
}

/// 用 refresh_token 静默换新 token（与 cockpit-tools refresh_access_token 一致）
pub fn refresh_access_token(refresh_token: &str) -> Result<TokenResponse, RefreshError> {
    #[cfg(debug_assertions)]
    let token_endpoint =
        std::env::var("HANGAR_TEST_TOKEN_ENDPOINT").unwrap_or_else(|_| TOKEN_ENDPOINT.to_string());
    #[cfg(not(debug_assertions))]
    let token_endpoint = TOKEN_ENDPOINT.to_string();
    let response = http_agent()
        .post(&token_endpoint)
        .send_form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .map_err(|error| match error {
            ureq::Error::Status(status, response) => {
                let body = response.into_string().unwrap_or_default();
                RefreshError::AuthRejected {
                    status,
                    code: extract_token_error_code(&body),
                    body_len: body.len(),
                }
            }
            other => RefreshError::Transport(format!("Token 刷新请求失败: {other}")),
        })?;
    response
        .into_json::<TokenResponse>()
        .map_err(|error| RefreshError::InvalidResponse(format!("解析 Token 刷新响应失败: {error}")))
}

/// 统一处理 ureq 请求：非 2xx 时只带状态码 + error_code + body_len，
/// 不回显完整 body（防 token/敏感信息打屏；对齐 cockpit extract_token_error_code）
fn extract_token_error_code(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("error")
        .and_then(|e| e.as_str())
        .or_else(|| {
            v.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_str())
        })
        .or_else(|| v.get("code").and_then(|c| c.as_str()))
        .map(|s| s.to_string())
}

/// 带超时的 HTTP 客户端：默认 ureq 无超时，网络黑洞会卡死 CLI（且 switch 持锁横跨请求，
/// 会连带阻塞第二实例至锁过期）。对齐 cockpit 25s 请求超时
fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .build()
}

fn send_form_checked(
    url: &str,
    form: &[(&str, &str)],
    ctx: &str,
) -> Result<ureq::Response, String> {
    http_agent().post(url).send_form(form).map_err(|e| match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let mut msg = format!("{} 失败: HTTP {}", ctx, code);
            if let Some(c) = extract_token_error_code(&body) {
                msg.push_str(&format!(", error_code={}", c));
            }
            msg.push_str(&format!(", body_len={}", body.len()));
            msg
        }
        other => format!("{} 失败: {}", ctx, other),
    })
}

fn exchange_code_for_token(
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    send_form_checked(
        TOKEN_ENDPOINT,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", code_verifier),
        ],
        "Token 请求",
    )?
    .into_json::<TokenResponse>()
    .map_err(|e| format!("解析 Token 响应失败: {}", e))
}

fn get_user_email(access_token: &str) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct UserInfo {
        email: Option<String>,
        #[allow(dead_code)]
        name: Option<String>,
    }

    // userinfo 为 OIDC 标准路径（auth.openai.com 为 Auth0 系，/oauth/userinfo 无依据）
    // 脱敏：与 send_form_checked 一致只记 status+error_code+body_len，不回显 body 防 token 泄漏
    let info: UserInfo = http_agent()
        .get("https://auth.openai.com/userinfo")
        .set("Authorization", &format!("Bearer {}", access_token))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                let mut msg = format!("获取用户信息失败: HTTP {}", code);
                if let Some(c) = extract_token_error_code(&body) {
                    msg.push_str(&format!(", error_code={}", c));
                }
                msg.push_str(&format!(", body_len={}", body.len()));
                msg
            }
            other => format!("获取用户信息失败: {}", other),
        })?
        .into_json()
        .map_err(|e| format!("解析用户信息失败: {}", e))?;

    // 缺 email 直接报错终止登录，禁止编造 unknown@codex 占位身份污染账号库
    info.email
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "userinfo 未返回 email，无法确定账号身份".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_reject_wrong_state() {
        struct StdinHooks;
        impl LoginHooks for StdinHooks {
            fn show_auth_url(&self, url: &str) {
                crate::emit::emit_err(url.to_string());
            }
            fn prompt_callback(&self, _state: &str) -> Option<String> {
                None
            }
        }
        let hooks = StdinHooks;
        assert!(parse_code_from_callback_url(
            "http://localhost:1455/auth/callback?code=abc&state=s1",
            "other"
        )
        .is_err());
        let _ = &hooks as &dyn LoginHooks;
    }

    #[test]
    fn callback_url_requires_localhost_auth_path() {
        let ok = "http://localhost:1455/auth/callback?code=abc&state=s1";
        assert_eq!(parse_code_from_callback_url(ok, "s1").as_deref(), Ok("abc"));
        assert!(parse_code_from_callback_url(ok, "other").is_err());
        // 旧 /callback 路径与非本地 host 均拒绝
        assert!(parse_code_from_callback_url(
            "http://127.0.0.1:1455/callback?code=abc&state=s1",
            "s1"
        )
        .is_err());
        assert!(parse_code_from_callback_url(
            "https://chatgpt.com/codex/desktop-auth?code=abc&state=s1",
            "s1"
        )
        .is_err());
    }

    #[test]
    fn token_error_code_extraction_redacts_body() {
        assert_eq!(
            extract_token_error_code(r#"{"error":"invalid_grant"}"#).as_deref(),
            Some("invalid_grant")
        );
        assert_eq!(
            extract_token_error_code(r#"{"error":{"code":"refresh_token_reused"}}"#).as_deref(),
            Some("refresh_token_reused")
        );
        assert!(extract_token_error_code("not json").is_none());
    }

    #[test]
    fn refresh_error_classifies_auth_without_string_matching() {
        let auth = RefreshError::AuthRejected {
            status: 400,
            code: Some("invalid_grant".to_string()),
            body_len: 123,
        };
        assert!(auth.is_auth_rejection());
        assert!(!auth.to_string().contains("secret"));
        assert!(!RefreshError::AuthRejected {
            status: 400,
            code: Some("temporarily_unavailable".to_string()),
            body_len: 0,
        }
        .is_auth_rejection());
        assert!(!RefreshError::Transport("timeout".to_string()).is_auth_rejection());
    }
}
