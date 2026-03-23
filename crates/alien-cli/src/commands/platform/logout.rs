use crate::auth::logout;
use crate::error::Result;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Remove saved tokens & workspace",
    long_about = "Log out from the Alien platform and remove saved authentication tokens and workspace settings."
)]
pub struct LogoutArgs {}

pub async fn logout_task(_args: LogoutArgs) -> Result<()> {
    logout();
    println!("Logged out!");
    Ok(())
}
