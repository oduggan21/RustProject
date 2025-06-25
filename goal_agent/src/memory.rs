use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum Status{
    New,
    Waiting, 
    InviteAccepted,
    Declined,
    Bounce,
    NotNow
}

impl Default for Status{ fn default() -> Self {Status::New}}

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct ProspectMem{
    pub name:    String,
    pub email:   String,
    pub company: String,
    pub role:    String,

    pub last_msg: Option<String>,
    pub last_stamp: Option<DateTime<Utc>>,

    pub prospect_replies: Vec<String>,

    pub status: Status,
    pub follow_ups: u8,
}
