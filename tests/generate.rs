//! Smoke test: every built-in template set generates a client for the example spec.

use oas2client::generate::{Request, run};
use std::path::PathBuf;

fn generate(set: &str) -> (PathBuf, Vec<String>) {
    let out = std::env::temp_dir().join(format!("oas2client-test-{set}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let files = run(&Request {
        spec: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/petstore.json"),
        output: out.clone(),
        set: set.into(),
        overrides: None,
        options: Default::default(),
    })
    .unwrap_or_else(|e| panic!("{set}: {e}"));
    (out, files)
}

#[test]
fn typescript_fetch() {
    let (out, files) = generate("typescript-fetch");
    assert!(files.contains(&"apis/pets-api.ts".to_string()));
    let model = std::fs::read_to_string(out.join("models/pet.ts")).unwrap();
    assert!(model.contains("export interface Pet {"));
    assert!(model.contains("status: PetStatus;"));
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
fn java() {
    let (out, files) = generate("java");
    assert!(files.contains(&"src/main/java/org/openapitools/client/api/PetsApi.java".to_string()));
    let model =
        std::fs::read_to_string(out.join("src/main/java/org/openapitools/client/model/Pet.java"))
            .unwrap();
    assert!(model.contains("private UUID id;"));
    assert!(model.contains("private List<String> tags = new ArrayList<>();"));
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
fn rejects_unresolvable_ref() {
    let dir = std::env::temp_dir().join(format!("oas2client-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let spec = dir.join("bad.json");
    std::fs::write(
        &spec,
        r##"{"openapi":"3.0.3","info":{"title":"t","version":"1"},"paths":{},
            "components":{"schemas":{"A":{"type":"object","properties":{"b":{"$ref":"#/components/schemas/Missing"}}}}}}"##,
    )
    .unwrap();
    let err = run(&Request {
        spec,
        output: dir.join("out"),
        set: "java".into(),
        overrides: None,
        options: Default::default(),
    })
    .unwrap_err();
    assert!(err.contains("unresolvable $ref"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}
