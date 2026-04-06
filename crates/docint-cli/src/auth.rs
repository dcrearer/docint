use anyhow::{Context, Result, bail};
use aws_sdk_cognitoidentityprovider::Client;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct Session {
    pub tenant_id: String,
    pub username: String,
}

#[derive(Serialize, Deserialize)]
struct TokenCache {
    id_token: String,
    refresh_token: String,
    client_id: String,
}

fn cache_path() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".docint");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("tokens.json"))
}

fn decode_jwt_payload(token: &str) -> Result<serde_json::Value> {
    let payload = token.split('.').nth(1).context("Invalid JWT")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn session_from_token(id_token: &str) -> Result<Session> {
    let claims = decode_jwt_payload(id_token)?;
    Ok(Session {
        tenant_id: claims["sub"].as_str().context("Missing sub claim")?.to_string(),
        username: claims["cognito:username"]
            .as_str()
            .context("Missing username claim")?
            .to_string(),
    })
}

fn is_expired(id_token: &str) -> bool {
    decode_jwt_payload(id_token)
        .ok()
        .and_then(|c| c["exp"].as_u64())
        .map(|exp| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            now >= exp
        })
        .unwrap_or(true)
}

async fn refresh_tokens(client: &Client, cache: &mut TokenCache) -> Result<String> {
    let resp = client
        .initiate_auth()
        .auth_flow(aws_sdk_cognitoidentityprovider::types::AuthFlowType::RefreshTokenAuth)
        .client_id(&cache.client_id)
        .auth_parameters("REFRESH_TOKEN", &cache.refresh_token)
        .send()
        .await
        .context("Token refresh failed")?;

    let result = resp.authentication_result().context("No auth result")?;
    let id_token = result.id_token().context("No id_token")?.to_string();
    cache.id_token = id_token.clone();
    std::fs::write(cache_path()?, serde_json::to_string(cache)?)?;
    Ok(id_token)
}

pub async fn try_restore_session(client: &Client, client_id: &str) -> Option<Session> {
    let path = cache_path().ok()?;
    let data = std::fs::read_to_string(&path).ok()?;
    let mut cache: TokenCache = serde_json::from_str(&data).ok()?;

    if cache.client_id != client_id {
        return None;
    }

    let id_token = if is_expired(&cache.id_token) {
        refresh_tokens(client, &mut cache).await.ok()?
    } else {
        cache.id_token.clone()
    };

    session_from_token(&id_token).ok()
}

async fn authenticate(client: &Client, client_id: &str, username: &str, password: &str) -> Result<Session> {
    let resp = client
        .initiate_auth()
        .auth_flow(aws_sdk_cognitoidentityprovider::types::AuthFlowType::UserPasswordAuth)
        .client_id(client_id)
        .auth_parameters("USERNAME", username)
        .auth_parameters("PASSWORD", password)
        .send()
        .await
        .context("Login failed — check username and password")?;

    let result = resp.authentication_result().context("No auth result")?;
    let id_token = result.id_token().context("No id_token")?;
    let refresh_token = result.refresh_token().context("No refresh_token")?;

    let cache = TokenCache {
        id_token: id_token.to_string(),
        refresh_token: refresh_token.to_string(),
        client_id: client_id.to_string(),
    };
    std::fs::write(cache_path()?, serde_json::to_string(&cache)?)?;

    session_from_token(id_token)
}

pub async fn login(client: &Client, client_id: &str) -> Result<Session> {
    let username = dialoguer::Input::<String>::new()
        .with_prompt("Username")
        .interact_text()?;

    let password = rpassword::prompt_password("Password: ")?;

    authenticate(client, client_id, &username, &password).await
}

pub async fn signup(client: &Client, client_id: &str) -> Result<Session> {
    let username = dialoguer::Input::<String>::new()
        .with_prompt("Username")
        .interact_text()?;

    let password = rpassword::prompt_password("Password: ")?;
    let confirm = rpassword::prompt_password("Confirm password: ")?;

    if password != confirm {
        bail!("Passwords do not match");
    }

    client
        .sign_up()
        .client_id(client_id)
        .username(&username)
        .password(&password)
        .send()
        .await
        .context("Sign up failed")?;

    eprintln!("✓ Account created. Logging you in...");
    authenticate(client, client_id, &username, &password).await
}

pub fn logout() -> Result<()> {
    let path = cache_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}
