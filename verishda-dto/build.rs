
 fn main() {
   // progenitor generated code
   run_progenitor();
 }
 
fn run_progenitor() {
    let src = "../verishda.yaml";
    println!("cargo:rerun-if-changed={src}");
    let file = std::fs::File::open(src).unwrap();
    let spec = serde_yaml::from_reader(file).unwrap();

    let settings = progenitor::GenerationSettings::new();
    
    let mut generator = progenitor::Generator::new(&settings);

    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let mut out_file = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("codegen_progenitor.rs");

    std::fs::write(out_file, content).unwrap();
}