use embed_manifest::{embed_manifest, manifest::ManifestBuilder, new_manifest};

fn main() {
    // slint build
    slint_build::compile("ui/ui.slint").unwrap();

    // progenitor generated code
    run_progenitor();

    do_embed_manifest();
}

fn run_progenitor() {
    let src = "openapi.yaml";
    println!("cargo:rerun-if-changed={src}");
    let file = std::fs::File::open(src).unwrap();
    let spec = serde_yaml::from_reader(file).unwrap();

    let settings = progenitor::GenerationSettings::new();
    //settings.with_pre_hook_async(quote!(client_pre_hook));
    
    let mut generator = progenitor::Generator::new(&settings);

    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let mut out_file = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("codegen_progenitor.rs");

    std::fs::write(out_file, content).unwrap();
}

// https://dev.to/carey/embed-a-windows-manifest-in-your-rust-program-26j2
fn do_embed_manifest() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Verishda"))
            .expect("unable to embed manifest file");
    } 
}