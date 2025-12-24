use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use std::path::Path;
use syn::Ident;

/// This will create a test function for each `.md` file in the directory.
/// Each test parses the markdown, converts back to markdown, and asserts equality.
#[proc_macro]
pub fn generate_markdown_tests(input: TokenStream) -> TokenStream {
    let fixtures_path = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join(input.to_string().trim_matches('"'));
    if !fixtures_path.exists() {
        panic!(
            "Fixtures directory does not exist: {}",
            fixtures_path.display()
        );
    }
    let pattern = fixtures_path.join("*.md");
    let tests = glob::glob(pattern.to_str().unwrap())
        .expect("Failed to read glob pattern")
        .filter_map(Result::ok)
        .map(|path| {
            let file_name = path.file_stem().unwrap().to_str().unwrap();
            let test_name = Ident::new(&file_name.replace('-', "_"), Span::call_site());
            let file_path = path.to_str().unwrap().to_string();

            quote! {
                #[test]
                fn #test_name() {
                    let input = include_str!(#file_path);
                    let doc = writ::document::Document::from_markdown(input);
                    let output = doc.to_markdown();
                    assert_eq!(input, output, "Roundtrip failed for {}", #file_path);
                }
            }
        })
        .collect::<Vec<_>>();

    quote! {
        #(#tests)*
    }
    .into()
}
