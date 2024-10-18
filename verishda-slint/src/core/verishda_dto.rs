use reqwest::StatusCode;
use tokio::sync::mpsc::Sender;


#[derive(Clone,Debug)]
pub struct ClientInner {
    cmd_tx: Sender<super::AppCoreCommand>
}

impl ClientInner {
    pub(super) fn new(cmd_tx: Sender<super::AppCoreCommand>) -> Self {
        Self {cmd_tx}
    }

    async fn post_hook(&self, result: &Result<reqwest::Response,reqwest::Error>) -> Result<(), &reqwest::Error>{
        match result {
    
            Ok(response) => {
                if StatusCode::UNAUTHORIZED == response.status() {
                    self.cmd_tx.send(super::AppCoreCommand::Logout).await.unwrap();
                }
            }

            Err(e) => {
                log::error!("error {e:?}");

                let connection_error = std::error::Error::source(e)
                    .map(|src|src.downcast_ref::<hyper_util::client::legacy::Error>())
                    .flatten()
                    .map(|hyper_error| hyper_error.is_connect())
                    .unwrap_or(false);

                if connection_error {
                    log::info!("DISCONNECTED");
                    self.cmd_tx.send(super::AppCoreCommand::StartTokenRefresh).await.unwrap();
                }
            }
        }
        Ok(())
    }
}
// https://github.com/oxidecomputer/progenitor
include!(concat!(env!("OUT_DIR"), "/codegen_progenitor.rs"));

