use anyhow::{Result, anyhow};
use collections::BTreeMap;
use futures::{FutureExt, StreamExt, future, future::BoxFuture};
use gpui::{AnyView, App, AsyncApp, Context, Entity, Task, Window};
use http_client::HttpClient;
use language_model::{
    AuthenticateError, IconOrSvg, LanguageModel, LanguageModelCompletionError,
    LanguageModelCompletionEvent, LanguageModelId, LanguageModelName, LanguageModelProvider,
    LanguageModelProviderId, LanguageModelProviderName, LanguageModelProviderState,
    LanguageModelRequest, LanguageModelToolChoice, LanguageModelToolSchemaFormat, RateLimiter,
    Role,
};
use open_ai::ResponseStreamEvent;
use qwen::Model;
use strum::IntoEnumIterator;
use settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use ui::{ButtonLink, ConfiguredApiCard, List, ListBulletItem, prelude::*};
use util::ResultExt;

const PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("qwen");
const PROVIDER_NAME: LanguageModelProviderName = LanguageModelProviderName::new("Qwen");

pub const QWEN_OAUTH_BASE_URL: &str = "https://chat.qwen.ai";
pub const QWEN_OAUTH_TOKEN_ENDPOINT: &str = "https://chat.qwen.ai/api/v1/oauth2/token";
pub const QWEN_OAUTH_CLIENT_ID: &str = "f0304373b74a44d2b584a3fb70ca9e56";

#[derive(thiserror::Error, Debug)]
pub enum QwenError {
    #[error("OAuth credentials file not found at {0}")]
    CredentialsNotFound(PathBuf),
    #[error("Invalid credentials format: {0}")]
    InvalidCredentials(String),
    #[error("Token refresh failed: {0}")]
    TokenRefreshFailed(String),
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] http_client::http::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QwenOAuthCredentials {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expiry_date: i64, // Unix timestamp in milliseconds
    pub resource_url: Option<String>,
}

impl QwenOAuthCredentials {
    pub fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let buffer = 30 * 1000; // 30 seconds buffer
        now >= self.expiry_date - buffer
    }
}

#[derive(Debug, Clone)]
pub struct QwenAuthClient {
    credentials_path: PathBuf,
    credentials: Arc<smol::lock::RwLock<Option<QwenOAuthCredentials>>>,
}

impl QwenAuthClient {
    pub fn new() -> Self {
        Self::with_path(None)
    }

    pub fn with_path(path: Option<PathBuf>) -> Self {
        let credentials_path = path.unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".qwen/oauth_creds.json")
        });

        Self {
            credentials_path,
            credentials: Arc::new(smol::lock::RwLock::new(None)),
        }
    }

    pub async fn load_credentials(&self) -> Result<QwenOAuthCredentials, QwenError> {
        let content = smol::fs::read_to_string(&self.credentials_path).await
            .map_err(|_| QwenError::CredentialsNotFound(self.credentials_path.clone()))?;

        let credentials: QwenOAuthCredentials = serde_json::from_str(&content)
            .map_err(|e| QwenError::InvalidCredentials(e.to_string()))?;

        Ok(credentials)
    }

    pub async fn get_valid_credentials(&self) -> Result<QwenOAuthCredentials, QwenError> {
        // Check if we have cached credentials
        {
            let cached = self.credentials.read().await;
            if let Some(ref creds) = *cached {
                if !creds.is_expired() {
                    return Ok(creds.clone());
                }
            }
        }

        // Load from file
        let mut credentials = self.load_credentials().await?;

        // Refresh if expired
        if credentials.is_expired() {
            credentials = self.refresh_token(&credentials).await?;
        }

        // Cache the credentials
        {
            let mut cached = self.credentials.write().await;
            *cached = Some(credentials.clone());
        }

        Ok(credentials)
    }

    async fn refresh_token(&self, _credentials: &QwenOAuthCredentials) -> Result<QwenOAuthCredentials, QwenError> {
        // Token refresh should be handled at the provider level where HTTP client is available
        // This is a placeholder that returns the current credentials to avoid breaking the flow
        // The actual refresh will be implemented in the stream_completion method
        Err(QwenError::TokenRefreshFailed("Token refresh not yet implemented in auth client".to_string()))
    }

    async fn save_credentials(&self, credentials: &QwenOAuthCredentials) -> Result<(), QwenError> {
        let content = serde_json::to_string_pretty(credentials)?;
        smol::fs::write(&self.credentials_path, content).await?;
        Ok(())
    }

    pub fn get_base_url(credentials: &QwenOAuthCredentials) -> String {
        let base_url = credentials.resource_url.as_deref()
            .unwrap_or("https://dashscope.aliyuncs.com/compatible-mode/v1");

        let mut url = base_url.to_string();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            url = format!("https://{}", url);
        }
        if !url.ends_with("/v1") {
            url = format!("{}/v1", url);
        }
        url
    }
}

#[derive(Default, Clone, Debug, PartialEq)]
pub struct QwenSettings {
    pub available_models: Vec<AvailableModel>,
}

pub struct QwenLanguageModelProvider {
    http_client: Arc<dyn HttpClient>,
    state: Entity<State>,
}

pub struct State {
    auth_client: QwenAuthClient,
    authenticated: bool,
    error: Option<String>,
}

impl State {
    fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    fn authenticate(&mut self, cx: &mut Context<Self>) -> Task<Result<(), AuthenticateError>> {
        let auth_client = self.auth_client.clone();
        cx.spawn(async move |entity, cx| {
            match auth_client.get_valid_credentials().await {
                Ok(_) => {
                    entity.update(cx, |state, _| {
                        state.authenticated = true;
                        state.error = None;
                    })?;
                    Ok(())
                }
                Err(QwenError::CredentialsNotFound(path)) => {
                    entity.update(cx, |state, _| {
                        state.authenticated = false;
                        state.error = Some(format!("OAuth credentials file not found at {}", path.display()));
                    })?;
                    Err(AuthenticateError::Other(anyhow!("Credentials not found")))
                }
                Err(err) => {
                    entity.update(cx, |state, _| {
                        state.authenticated = false;
                        state.error = Some(err.to_string());
                    })?;
                    Err(AuthenticateError::Other(anyhow!("Authentication failed: {}", err)))
                }
            }
        })
    }
}

impl QwenLanguageModelProvider {
    pub fn new(http_client: Arc<dyn HttpClient>, cx: &mut App) -> Self {
        let state = cx.new(|_| State {
            auth_client: QwenAuthClient::new(),
            authenticated: false,
            error: None,
        });

        Self { http_client, state }
    }

    fn create_language_model(&self, model: Model) -> Arc<dyn LanguageModel> {
        Arc::new(QwenLanguageModel {
            id: LanguageModelId::from(model.id().to_string()),
            model,
            state: self.state.clone(),
            http_client: self.http_client.clone(),
            request_limiter: RateLimiter::new(4),
        })
    }

    fn settings(cx: &App) -> &QwenSettings {
        &crate::AllLanguageModelSettings::get_global(cx).qwen
    }
}

impl LanguageModelProviderState for QwenLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for QwenLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::Ai)
    }

    fn default_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        Some(self.create_language_model(Model::default()))
    }

    fn default_fast_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        Some(self.create_language_model(Model::default_fast()))
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        let mut models = BTreeMap::default();

        for model in Model::iter() {
            if !matches!(model, Model::Custom { .. }) {
                models.insert(model.id().to_string(), model);
            }
        }

        for model in &Self::settings(cx).available_models {
            models.insert(
                model.name.clone(),
                Model::Custom {
                    name: model.name.clone(),
                    display_name: model.display_name.clone(),
                    max_tokens: model.max_tokens,
                    max_output_tokens: model.max_output_tokens,
                    max_completion_tokens: model.max_completion_tokens,
                    supports_images: model.supports_images,
                    supports_tools: model.supports_tools,
                    parallel_tool_calls: model.parallel_tool_calls,
                },
            );
        }

        models
            .into_values()
            .map(|model| self.create_language_model(model))
            .collect()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        self.state.read(cx).is_authenticated()
    }

    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>> {
        self.state.update(cx, |state, cx| state.authenticate(cx))
    }

    fn configuration_view(
        &self,
        _target_agent: language_model::ConfigurationViewTargetAgent,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyView {
        cx.new(|cx| ConfigurationView::new(self.state.clone(), window, cx))
            .into()
    }

    fn reset_credentials(&self, cx: &mut App) -> Task<Result<()>> {
        // For OAuth, we don't reset credentials but can clear the authentication state
        self.state.update(cx, |state, _| {
            state.authenticated = false;
            state.error = None;
        });
        Task::ready(Ok(()))
    }
}

pub struct QwenLanguageModel {
    id: LanguageModelId,
    model: Model,
    state: Entity<State>,
    http_client: Arc<dyn HttpClient>,
    request_limiter: RateLimiter,
}

impl QwenLanguageModel {
    fn stream_completion(
        &self,
        request: open_ai::Request,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<'static, Result<ResponseStreamEvent>>,
            LanguageModelCompletionError,
        >,
    > {
        let http_client = self.http_client.clone();

        let Ok(task) = self.state.read_with(cx, |state, cx| {
            let auth_client = state.auth_client.clone();
            cx.background_spawn(async move {
                auth_client.get_valid_credentials().await
                    .map(|creds| {
                        let base_url = QwenAuthClient::get_base_url(&creds);
                        (creds, base_url)
                    })
            })
        }) else {
            return future::ready(Err(anyhow!("App state dropped").into())).boxed();
        };

        let future = self.request_limiter.stream(async move {
            let (credentials, base_url) = task.await.map_err(|e| LanguageModelCompletionError::Other(anyhow!(e)))?;

            let provider = PROVIDER_NAME;
            let request = open_ai::stream_completion(
                http_client.as_ref(),
                provider.0.as_str(),
                &base_url,
                &credentials.access_token,
                request,
            );
            let response = request.await?;
            Ok(response)
        });

        async move { Ok(future.await?.boxed()) }.boxed()
    }
}

impl LanguageModel for QwenLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        LanguageModelName::from(self.model.display_name().to_string())
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn supports_tools(&self) -> bool {
        self.model.supports_tool()
    }

    fn supports_images(&self) -> bool {
        self.model.supports_images()
    }

    fn supports_tool_choice(&self, choice: LanguageModelToolChoice) -> bool {
        match choice {
            LanguageModelToolChoice::Auto
            | LanguageModelToolChoice::Any
            | LanguageModelToolChoice::None => true,
        }
    }

    fn tool_input_format(&self) -> LanguageModelToolSchemaFormat {
        LanguageModelToolSchemaFormat::JsonSchema
    }

    fn telemetry_id(&self) -> String {
        format!("qwen/{}", self.model.id())
    }

    fn max_token_count(&self) -> u64 {
        self.model.max_token_count()
    }

    fn max_output_tokens(&self) -> Option<u64> {
        self.model.max_output_tokens()
    }

    fn count_tokens(
        &self,
        request: LanguageModelRequest,
        cx: &App,
    ) -> BoxFuture<'static, Result<u64>> {
        count_qwen_tokens(request, self.model.clone(), cx)
    }

    fn stream_completion(
        &self,
        request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<
                'static,
                Result<LanguageModelCompletionEvent, LanguageModelCompletionError>,
            >,
            LanguageModelCompletionError,
        >,
    > {
        let request = crate::provider::open_ai::into_open_ai(
            request,
            self.model.id(),
            self.model.supports_parallel_tool_calls(),
            self.model.supports_prompt_cache_key(),
            self.max_output_tokens(),
            None,
        );
        let completions = self.stream_completion(request, cx);
        async move {
            let mapper = crate::provider::open_ai::OpenAiEventMapper::new();
            Ok(mapper.map_stream(completions.await?).boxed())
        }
        .boxed()
    }
}

pub fn count_qwen_tokens(
    request: LanguageModelRequest,
    model: Model,
    cx: &App,
) -> BoxFuture<'static, Result<u64>> {
    cx.background_spawn(async move {
        let messages = request
            .messages
            .into_iter()
            .map(|message| tiktoken_rs::ChatCompletionRequestMessage {
                role: match message.role {
                    Role::User => "user".into(),
                    Role::Assistant => "assistant".into(),
                    Role::System => "system".into(),
                },
                content: Some(message.string_contents()),
                name: None,
                function_call: None,
            })
            .collect::<Vec<_>>();

        // Use appropriate model for token counting
        let model_name = if model.max_token_count() >= 100_000 {
            "gpt-4o"
        } else {
            "gpt-4"
        };
        tiktoken_rs::num_tokens_from_messages(model_name, &messages).map(|tokens| tokens as u64)
    })
    .boxed()
}

struct ConfigurationView {
    state: Entity<State>,
    load_credentials_task: Option<Task<()>>,
}

impl ConfigurationView {
    fn new(state: Entity<State>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| {
            cx.notify();
        })
        .detach();

        let load_credentials_task = Some(cx.spawn_in(window, {
            let state = state.clone();
            async move |this, cx| {
                if let Some(task) = state
                    .update(cx, |state, cx| state.authenticate(cx))
                    .log_err()
                {
                    let _ = task.await;
                }
                this.update(cx, |this, cx| {
                    this.load_credentials_task = None;
                    cx.notify();
                })
                .log_err();
            }
        }));

        Self {
            state,
            load_credentials_task,
        }
    }
}

impl Render for ConfigurationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);

        if self.load_credentials_task.is_some() {
            return div().child(Label::new("Loading credentials…")).into_any();
        }

        if state.authenticated {
            ConfiguredApiCard::new("OAuth authentication configured")
                .on_click(cx.listener(|this, _, window, cx| {
                    let state = this.state.clone();
                    cx.spawn_in(window, async move |_, cx| {
                        state.update(cx, |state, _| {
                            state.authenticated = false;
                            state.error = None;
                        }).log_err();
                    }).detach();
                }))
                .into_any_element()
        } else {
            let error_message = state.error.as_deref().unwrap_or("OAuth authentication required").to_string();

            v_flex()
                .child(Label::new("To use Zed's agent with Qwen, you need to authenticate with OAuth:"))
                .child(
                    List::new()
                        .child(
                            ListBulletItem::new("")
                                .child(Label::new("Install and authenticate the Qwen client"))
                                .child(ButtonLink::new("Qwen Client", "https://github.com/qwen-app/qwen-client"))
                        )
                        .child(
                            ListBulletItem::new("Ensure credentials exist at ~/.qwen/oauth_creds.json")
                        ),
                )
                .child(
                    Label::new(error_message)
                        .color(Color::Error)
                )
                .child(
                    Label::new("Note: Qwen uses OAuth authentication instead of API keys.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any()
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AvailableModel {
    pub name: String,
    pub display_name: Option<String>,
    pub max_tokens: u64,
    pub max_output_tokens: Option<u64>,
    pub max_completion_tokens: Option<u64>,
    pub supports_images: Option<bool>,
    pub supports_tools: Option<bool>,
    pub parallel_tool_calls: Option<bool>,
}
