
// https://github.com/oxidecomputer/progenitor
include!(concat!(env!("OUT_DIR"), "/codegen_progenitor.rs"));


async fn client_pre_hook(client: reqwest::Client, req: reqwest::Request) -> reqwest::Request {
    let mut req = req;
    req.headers_mut().insert("x-client-id", "verishda-windows".parse().unwrap());
    req
}
