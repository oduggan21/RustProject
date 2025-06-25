
use tokio::time::{sleep, Duration};

use crate::memory::ProspectMem;
use crate::email_agent::{Action, Target};

pub async fn run_goal<A, T>(
    task_name: String,
    interval: u64,
    mut mem: ProspectMem,
    action: A,
    target: T,
)
where
    A: Action + 'static,
    T: Target + 'static,
{
    loop {
        if target.met(&mem) {
            println!("{task_name} completed");
            break;
        }
        if let Err(e) = action.run(&mut mem).await {
            eprintln!("{task_name}: {e}");     
        }
        sleep(Duration::from_secs(interval)).await;
    }
}
