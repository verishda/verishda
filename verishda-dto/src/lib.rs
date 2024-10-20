use proc_macro2::TokenStream;


 
pub fn run_progenitor(openapi_path: &str, inner_type: TokenStream, post_hook: Option<TokenStream>) {
    let src = openapi_path;
    println!("cargo:rerun-if-changed={src}");
    let file = std::fs::File::open(src).unwrap();
    let spec = serde_yaml::from_reader(file).unwrap();

    let mut settings = progenitor::GenerationSettings::new();
    settings.with_inner_type(inner_type);
    if let Some(hook) = post_hook {
        settings.with_post_hook_async(hook);
    }

    let mut generator = progenitor::Generator::new(&settings);

    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let mut out_file = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("codegen_progenitor.rs");

    std::fs::write(out_file, content).unwrap();
}