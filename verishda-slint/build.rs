use embed_manifest::{embed_manifest, new_manifest};
use quote::quote;

fn main() {
    // slint build
    println!("cargo::rerun-if-changed=ui/icons");
    slint_build::compile("ui/ui.slint").unwrap();

    do_embed_manifest();

    do_embed_resources();

    verishda_dto::run_progenitor("../verishda.yaml", quote!(ClientInner));
}


// https://dev.to/carey/embed-a-windows-manifest-in-your-rust-program-26j2
fn do_embed_manifest() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Verishda"))
            .expect("unable to embed manifest file");
    } 
}

fn do_embed_resources() {
    embed_resource::compile("ui/icons/tray.rc", embed_resource::NONE);
}