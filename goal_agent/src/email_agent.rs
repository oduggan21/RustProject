// src/email_agent.rs ----------------------------------------------------------
use anyhow::{Context, Result};
use hyper::{client::HttpConnector, Client as HyperClient};
use hyper_rustls::HttpsConnector;
use async_trait::async_trait;
use async_openai::{config::OpenAIConfig, types::*, Client};

// crystal-clear crate aliases
use google_gmail1  as gmail1;
use google_calendar3 as cal3;
use google_gmail1::api::{Message, Scope as GmailScope};
use std::thread::Scope;
use google_calendar3::api::Scope as CalScope;
use chrono::{Duration, Timelike, Utc};
use cal3::api::{Event, EventAttendee, EventDateTime};

use std::io::Cursor;
use mime::Mime;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

// re-exported OAuth types
use gmail1::oauth2::{
    read_application_secret, InstalledFlowAuthenticator, InstalledFlowReturnMethod,
};

use crate::memory::{ProspectMem, Status};

// -----------------------------------------------------------------------------
// type aliases
type Https       = HttpsConnector<HttpConnector>;
type GMailHub    = gmail1::Gmail<Https>;
type CalendarHub = cal3::CalendarHub<Https>;
type OAClient    = Client<OpenAIConfig>;

// -----------------------------------------------------------------------------
// OAuth helpers
async fn gmail_hub() -> Result<GMailHub> {
    let secret = read_application_secret("credentials.json")
        .await
        .context("credentials.json missing")?;

    let auth = InstalledFlowAuthenticator::builder(
            secret,
            InstalledFlowReturnMethod::Interactive,
        )
        .persist_tokens_to_disk("tokencache.json")
        .build()
        .await?;

    let https  = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .build();
    let client = HyperClient::builder().build(https);

    Ok(gmail1::Gmail::new(client, auth))
}

async fn calendar_hub() -> Result<CalendarHub> {
    let secret = read_application_secret("credentials.json")
        .await
        .context("credentials.json missing")?;

    let auth = InstalledFlowAuthenticator::builder(
            secret,
            InstalledFlowReturnMethod::Interactive,
        )
        .persist_tokens_to_disk("tokencache.json")
        .build()
        .await?;

    let https  = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .build();
    let client = HyperClient::builder().build(https);

    Ok(cal3::CalendarHub::new(client, auth))
}

// -----------------------------------------------------------------------------
// Gmail helpers
async fn send_email(to: &str, body: &str) -> Result<()> {
    let rfc822 = format!(
        "To: {to}\r\n\
         Subject: Quick chat?\r\n\
         Content-Type: text/plain; charset=\"UTF-8\"\r\n\
         \r\n\
         {body}"
    );

    let msg     = Message::default();
    let reader  = Cursor::new(rfc822.into_bytes());
    let mime: Mime = "message/rfc822".parse().unwrap();

    gmail_hub()
        .await?
        .users()
        .messages_send(msg, "me")
        .upload(reader, mime)
        .await?;

    println!("sent follow-up to {to}");
    Ok(())
}

/// fetch last replies → full plain-text bodies
async fn fetch_new_replies(mem: &ProspectMem) -> Result<Vec<String>> {
    // a. query
    let after = mem.last_stamp
        .unwrap_or_else(|| Utc::now() - Duration::days(30))
        .format("%Y/%m/%d");
    let query = format!("from:{} after:{after}", mem.email);

    // b. list
    let hub            = gmail_hub().await?;
    let (_, list_resp) = hub.users()
        .messages_list("me")
        .q(&query)
        .max_results(5)
        .add_scope(GmailScope::Readonly)
        .doit()
        .await?;

    // c. bodies
    let mut out = Vec::new();
    if let Some(messages) = list_resp.messages {
        for m in messages {
            if let Some(id) = m.id {
                let (_, msg) = hub.users()
                    .messages_get("me", &id)
                    .format("full")
                    .add_scope(GmailScope::Readonly)
                    .doit()
                    .await?;

                let full = msg.payload
                    .as_ref()
                    .and_then(|p| p.body.as_ref().and_then(|b| b.data.as_ref()))
                    .or_else(|| msg.payload
                        .as_ref()
                        .and_then(|p| p.parts.as_ref())
                        .and_then(|parts| parts.iter()
                            .find(|part| part.mime_type
                                .as_deref()
                                .unwrap_or("")
                                .starts_with("text/plain"))
                            .and_then(|part| part.body
                                .as_ref()
                                .and_then(|b| b.data.as_ref()))));

                if let Some(b64) = full {
                    if let Ok(bytes) = URL_SAFE_NO_PAD.decode(b64) {
                        if let Ok(txt) = String::from_utf8(bytes) {
                            out.push(txt);
                            continue;
                        }
                    }
                }
                out.push(msg.snippet.unwrap_or_default());
            }
        }
    }
    Ok(out)
}

// -----------------------------------------------------------------------------
// OpenAI helpers
fn oa() -> OAClient {
    OAClient::with_config(
        OpenAIConfig::new()
            .with_api_key(std::env::var("OPENAI_API_KEY")
            .expect("OPENAI_API_KEY not set")),
    )
}

async fn analyze_reply(txt: &str) -> Status {
    let prompt = format!(
        "Classify this email strictly as one of \
         ACCEPTED, DECLINED, NOT_NOW, BOUNCE, OTHER:\n\n{txt}"
    );

    let req = CreateChatCompletionRequestArgs::default()
        .model("gpt-3.5-turbo")
        .messages([
            ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage {
                    content: "You are a helpful assistant.".into(),
                    ..Default::default()
                },
            ),
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(prompt),
                    ..Default::default()
                },
            ),
        ])
        .max_tokens(3u16)
        .build()
        .unwrap();

    let resp = oa().chat().create(req).await.unwrap();
    let content = resp.choices[0]
        .message
        .content
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_uppercase();

    match content.as_str() {
        "ACCEPTED" | "YES" | "SURE" => Status::InviteAccepted,
        "DECLINED" | "NO"          => Status::Declined,
        "BOUNCE"                   => Status::Bounce,
        "NOT_NOW"                  => Status::NotNow,
        _                          => Status::Waiting,
    }
}

async fn draft_follow_up(_mem: &ProspectMem) -> Result<String> {
    // unchanged …
    unimplemented!()
}

// -----------------------------------------------------------------------------
// Calendar helpers
async fn create_invite(mem: &ProspectMem) -> Result<()> {
    let start = (Utc::now() + Duration::days(2)).with_second(0).unwrap();
    let end   = start + Duration::minutes(15);

    let ev = Event {
        summary: Some("15-min intro chat".into()),
        attendees: Some(vec![EventAttendee {
            email: Some(mem.email.clone()),
            ..Default::default()
        }]),
        start: Some(EventDateTime {
            date_time: Some(start),
            time_zone: Some("UTC".into()),
            ..Default::default()
        }),
        end: Some(EventDateTime {
            date_time: Some(end),
            time_zone: Some("UTC".into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    calendar_hub()
        .await?
        .events()
        .insert(ev, "primary")
        .send_updates("all")
        .add_scope("https://www.googleapis.com/auth/calendar")
    // permission for write
        .doit()
        .await?;

    println!("Calendar invite sent to {}", mem.email);
    Ok(())
}

// -----------------------------------------------------------------------------
// traits & goal
#[async_trait]
pub trait Action { async fn run(&self, mem: &mut ProspectMem) -> Result<()>; }
pub trait Target  { fn met(&self, mem: &ProspectMem) -> bool; }

pub struct FollowUp;
#[async_trait]
impl Action for FollowUp {
    async fn run(&self, mem: &mut ProspectMem) -> Result<()> {
        // 1) harvest replies
        for reply in fetch_new_replies(mem).await? {
            mem.prospect_replies.push(reply.clone());
            mem.status = analyze_reply(&reply).await;

            if mem.status == Status::InviteAccepted {
                if let Err(e) = create_invite(mem).await {
                    eprintln!("❗ invite failed: {e:#}");
                }
            }
            if mem.status == Status::NotNow {
                mem.last_stamp = Some(Utc::now() + Duration::weeks(2));
            }
        }

        // 2) schedule follow-ups
        let now      = Utc::now();
        let last     = mem.last_stamp.unwrap_or(now - Duration::days(2));
        let elapsed  = now.signed_duration_since(last);

        if mem.status == Status::Waiting && elapsed > Duration::hours(48) {
            let body = draft_follow_up(mem).await?;
            mem.last_msg = Some(body.clone());
            send_email(&mem.email, &body).await?;

            mem.last_stamp = Some(now);
            mem.follow_ups += 1;
        }
        Ok(())
    }
}

pub struct InviteAccepted;
impl Target for InviteAccepted {
    fn met(&self, mem: &ProspectMem) -> bool {
        mem.status == Status::InviteAccepted
    }
}
