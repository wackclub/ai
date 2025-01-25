use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder, middleware::Logger};
use reqwest::Client;
use serde::{Serialize, Deserialize};

#[get("/")]
async fn hello() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
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
        content: String
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct RequestPayload {
        model: Option<String>,
        messages: Vec<ChatCompletionMessage>,
        stream: Option<bool>
    }

    #[derive(Deserialize, Debug)]
    pub struct Test {
        foo: String
    }

    pub async fn completions(data: web::Data<AppState>, body: web::Json<RequestPayload>) -> Result<impl Responder, Box<dyn std::error::Error>> {
        // let messages = serde_json::to_string(&body.messages)?;

        let res = data.client.post(COMPLETIONS_URL).json(&RequestPayload {
            model: Some("deepseek-chat".to_string()),
            messages: body.messages.clone(),
            stream: Some(false),
        }).send().await?;
        let status = res.status();
        let text = res.text().await?;
        println!("RES TEXT: {:#?}", text);
        Ok(text)
    }
}

struct AppState {
    client: Client,
}

#[actix_web::main]
pub async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("debug"));
    use reqwest::header;

    HttpServer::new(|| {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/json"));
        let bearer = format!("Bearer {}", std::env::var("KEY").expect("an API key"));
        let mut token = header::HeaderValue::from_str(&bearer).unwrap();
        token.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, token);
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("a successfully built client");
        let app_state = AppState { client };

        App::new()
            .app_data(web::Data::new(app_state))
            .service(hello)
            .service(echo)
            .route("/chat/completions", web::post().to(chat::completions))
            .route("/hey", web::get().to(manual_hello))
            .wrap(Logger::default())
    })
    .bind(("127.0.0.1", 8080))?
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
