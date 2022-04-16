use serde_json::json;
use worker::*;

mod utils;

fn log_request(req: &Request) {
    console_log!(
        "{} - [{}], located at: {:?}, within: {}",
        Date::now().to_string(),
        req.path(),
        req.cf().coordinates().unwrap_or_default(),
        req.cf().region().unwrap_or_else(|| "unknown region".into())
    );
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    log_request(&req);

    utils::set_panic_hook();

    let router = Router::new();

    // Add as many routes as your Worker needs! Each route will get a `Request` for handling HTTP
    // functionality and a `RouteContext` which you can use to  and get route parameters and
    // Environment bindings like KV Stores, Durable Objects, Secrets, and Variables.
    router
        .get_async("/users/:user_id", |mut _req, ctx| async move {
            if let Some(user_id) = ctx.param("user_id") {
                return Ok(
                    Response::from_bytes(fetch_articles(user_id).await?.into_bytes())?
                        .with_headers({
                            let mut headers = Headers::new();
                            headers.set("content-type", "application/rss+xml").unwrap();
                            headers
                        }),
                );
            }

            Response::error("Bad Request", 400)
        })
        .run(req, env)
        .await
}

async fn fetch_articles(user_id: &str) -> Result<String> {
    use chrono::serde::ts_milliseconds;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug)]
    struct Entry {
        id: String,
        author: String,
        #[serde(rename = "contentMd")]
        content_md: String,
        #[serde(with = "ts_milliseconds")]
        #[serde(rename = "createdAt")]
        created_at: DateTime<Utc>,
        #[serde(with = "ts_milliseconds")]
        #[serde(rename = "lastUpdatedAt")]
        last_updated_at: DateTime<Utc>,
    }
    #[derive(Deserialize, Debug)]
    struct FetchPublicDiariesResponse {
        result: Vec<Entry>,
    }

    let req = Request::new_with_init(
        "https://asia-northeast1-wywiwya.cloudfunctions.net/fetchPublicDiaries",
        RequestInit::new()
            .with_method(Method::Post)
            .with_cf_properties({
                let mut prop = CfProperties::new();
                prop.cache_ttl = Some(60);
                prop
            })
            .with_headers({
                let mut headers = Headers::new();
                headers.set("content-type", "application/json").unwrap();
                headers
            })
            .with_body(Some(wasm_bindgen::JsValue::from_str(
                &json!({"data": { "uid": user_id }}).to_string(),
            ))),
    )
    .unwrap();

    let res: FetchPublicDiariesResponse = Fetch::Request(req)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let channel = rss::ChannelBuilder::default()
        .title(format!("{} -- {}", user_id, "WYWIWYA"))
        .link(format!("https://wywiwya.smallkirby.xyz/users/{}", user_id))
        .last_build_date(
            res.result
                .last()
                .map(|r| r.last_updated_at)
                .unwrap_or_else(Utc::now)
                .to_rfc2822(),
        )
        .items(
            res.result
                .into_iter()
                .map(|it| {
                    let Entry {
                        author,
                        id,
                        content_md,
                        created_at,
                        ..
                    } = it;

                    rss::ItemBuilder::default()
                        .author(Some(author))
                        .link(format!("https://wywiwya.smallkirby.xyz/view/{}", id))
                        .description(Some(content_md))
                        .pub_date(created_at.to_rfc2822())
                        .build()
                })
                .collect::<Vec<rss::Item>>(),
        )
        .build();

    Ok(channel.to_string())
}
