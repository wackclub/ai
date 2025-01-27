use actix_files::NamedFile;
use actix_web::{get, middleware::Logger, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use actix_web::error::ErrorBadRequest;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use minijinja::{Environment, context, path_loader};

#[get("/")]
async fn index(data: web::Data<AppState>) -> Result<impl Responder, Box<dyn std::error::Error>> {
    let conn = data.db_pool.get().await.map_err(|e| {
        eprintln!("Failed to get DB connection: {:?}", e);
        actix_web::error::ErrorInternalServerError("Database error")
    })?;
    let sum: f32 = conn.query("SELECT SUM((response->'usage'->>'total_tokens')::real) FROM api_request_logs;", &[]).await?[0].get("sum");
    println!("{:#?}", sum as i32);

    let mut env = Environment::new();
    env.set_loader(path_loader("templates"));
    let tmpl = env.get_template("index.html")?;
    let page = tmpl.render(context!(total_tokens => (sum as i32)))?;

    Ok(HttpResponse::Ok().content_type("text/html").body(page))
}

#[post("/echo")]
async fn echo(req_body: String) -> impl Responder {
    HttpResponse::Ok().body(req_body)
}

async fn manual_hello() -> impl Responder {
    HttpResponse::Ok().body("Hey there!")
}

mod chat {
    use super::*;
    static COMPLETIONS_URL: &str = "https://api.deepseek.com/chat/completions";

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct ChatCompletionMessage {
        role: String,
        content: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct RequestPayload {
        model: Option<String>,
        messages: Vec<ChatCompletionMessage>,
        stream: Option<bool>,
    }

    pub async fn completions(
        data: web::Data<AppState>,
        body: web::Json<RequestPayload>,
        req: HttpRequest
    ) -> Result<impl Responder, Box<dyn std::error::Error>> {
        // let messages = serde_json::to_string(&body.messages)?;

        if let Some(peer_addr) = req.peer_addr() {
            println!("Address: {:?}", peer_addr.ip().to_string());
        }

        println!("{:?}", req.headers());

        let res = data
            .client
            .post(COMPLETIONS_URL)
            .json(&RequestPayload {
                model: Some("deepseek-chat".to_string()),
                messages: body.messages.clone(),
                stream: Some(false),
            })
            .send()
            .await?
            .json::<serde_json::Value>().await?;

        let conn = data.db_pool.get().await.map_err(|e| {
            eprintln!("Failed to get DB connection: {:?}", e);
            actix_web::error::ErrorInternalServerError("Database error")
        })?;

        let peer_ip = req.peer_addr()
            .ok_or_else(|| ErrorBadRequest("Client IP address not available"))?
            .ip();

        let user_agent = req.headers()
            .get("user-agent")
            .map(|v| v.to_str().ok());

        let qr = conn.query("INSERT INTO api_request_logs (request, response, ip, user_agent) VALUES ($1, $2, $3, $4)", &[&serde_json::to_value(body.into_inner())?, &res, &peer_ip, &user_agent]).await;
        println!("{:#?}", qr);

        Ok(web::Json(res))
    }
}

struct AppState {
    client: Client,
    db_pool: Pool
}

#[actix_web::main]
pub async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("debug"));

    //#region DB setup
    let mut db_cfg = Config::new();
    db_cfg.url = Some(std::env::var("DB_URL").expect("a Postgres URL").to_string());
    db_cfg.manager = Some(ManagerConfig { recycling_method: RecyclingMethod::Fast });
    let db_pool = db_cfg.create_pool(Some(Runtime::Tokio1), tokio_postgres::NoTls).unwrap();
    db_pool
        .get()
        .await
        .unwrap()
        .batch_execute("CREATE TABLE IF NOT EXISTS api_request_logs (
    id SERIAL PRIMARY KEY,
    request JSONB NOT NULL,
    response JSONB NOT NULL,
    ip INET NOT NULL,
    user_agent VARCHAR(512),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);")
        .await
        .unwrap();
    //#endregion

    HttpServer::new(move || {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        let bearer = format!("Bearer {}", std::env::var("KEY").expect("an API key"));
        let mut token = header::HeaderValue::from_str(&bearer).unwrap();
        token.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, token);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("a successfully built client");

        let app_state = AppState { client, db_pool: db_pool.clone() };

        App::new()
            .app_data(web::Data::new(app_state))
            .service(index)
            .service(echo)
            .route("/chat/completions", web::post().to(chat::completions))
            .route("/hey", web::get().to(manual_hello))
            .wrap(Logger::default())
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

/*
 * curl https://api.deepseek.com/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <DeepSeek API Key>" \
  -d '{
        "model": "deepseek-chat",
        "messages": [
          {"role": "system", "content": "You are a helpful assistant."},
          {"role": "user", "content": "Hello!"}
        ],
        "stream": false
      }'
*/
