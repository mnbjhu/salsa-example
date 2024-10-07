use std::collections::HashMap;

use salsa::{interned, Accumulator, Database, Setter, Storage, Update};

fn main() {
    let mut db = TestDatabase::default();
    let file1 = File::new(
        &db,
        "file1".to_string(),
        r#"abc
foo"#
            .to_string(),
    );
    let project = Project::new(
        &db,
        vec![
            file1,
            File::new(
                &db,
                "file2".to_string(),
                r#"file1.foo
                test"#
                    .to_string(),
            ),
        ],
    );
    // compile_project(&db, project);
    let diags = compile_project::accumulated::<Diagnostic>(&db, project);
    for diags in diags.iter() {
        println!("{}: {}", diags.file, diags.message);
    }
    println!("Second Pass");
    file1.set_content(&mut db).to(r#"file2
    foo"#
        .to_string());
    // compile_project(&db, project);
    let diags = compile_project::accumulated::<Diagnostic>(&db, project);
    for diags in diags.iter() {
        println!("{}: {}", diags.file, diags.message);
    }

    println!("Third Pass");
    file1.set_content(&mut db).to(r#"file2
    foo test"#
        .to_string());
    // compile_project(&db, project);
    let diags = compile_project::accumulated::<Diagnostic>(&db, project);
    for diags in diags.iter() {
        println!("{}: {}", diags.file, diags.message);
    }
}

#[salsa::db]
#[derive(Default)]
struct TestDatabase {
    storage: Storage<Self>,
}

#[salsa::db]
impl salsa::Database for TestDatabase {
    fn salsa_event(&self, _: &dyn Fn() -> salsa::Event) {}
}

#[salsa::input]
struct File {
    #[interned]
    name: String,

    #[return_ref]
    content: String,
}

#[salsa::tracked]
struct Module<'db> {
    #[id]
    #[interned]
    name: String,
    #[return_ref]
    inner: ModuleInner<'db>,
}

#[salsa::accumulator]
struct Diagnostic {
    message: String,
    file: String,
}

#[derive(Debug, Clone, Update)]
enum ModuleInner<'db> {
    Export,
    Dir(Vec<Module<'db>>),
}

#[salsa::tracked]
fn resolve<'db>(
    db: &'db dyn Database,
    module: Module<'db>,
    name: &'db [&'db str],
) -> Option<Module<'db>> {
    println!("resolving module: {:?}", name);
    if name.is_empty() {
        Some(module)
    } else if let ModuleInner::Dir(modules) = module.inner(db) {
        modules
            .iter()
            .find(|m| m.name(db) == name[0])
            .and_then(|m| resolve(db, *m, &name[1..]))
    } else {
        None
    }
}

#[salsa::input]
struct Project {
    files: Vec<File>,
}

#[salsa::tracked]
struct Ast<'db> {
    #[return_ref]
    imports: Vec<String>,
    #[return_ref]
    exports: Vec<String>,
}

#[salsa::tracked]
fn parse<'db>(db: &'db dyn Database, file: File) -> Ast<'db> {
    println!("parsing file: {}", file.name(db));
    let lines = file.content(db).lines().collect::<Vec<_>>();
    assert!(lines.len() == 2, "invalid file\n{:?}", file.content(db));
    let imports = lines[0]
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let exports = lines[1]
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    Ast::new(db, imports, exports)
}

#[salsa::tracked]
fn get_file_module<'db>(db: &'db dyn Database, file: File) -> Module<'db> {
    println!("getting module for file: {}", file.name(db));
    let ast = parse(db, file);
    let exports = ast
        .exports(db)
        .iter()
        .map(|s| Module::new(db, s.to_string(), ModuleInner::Export))
        .collect::<Vec<_>>();
    Module::new(db, file.name(db), ModuleInner::Dir(exports))
}

#[salsa::tracked]
fn get_project_module<'db>(db: &'db dyn Database, project: Project) -> Module<'db> {
    println!("getting module for project");
    let modules = project
        .files(db)
        .iter()
        .map(|file| get_file_module(db, *file))
        .collect::<Vec<_>>();
    Module::new(db, "root".to_string(), ModuleInner::Dir(modules))
}

#[salsa::tracked]
fn check_file<'db>(db: &'db dyn Database, module: Module<'db>, file: File) {
    println!("checking file: {}", file.name(db));
    let ast = parse(db, file);
    let imports = ast
        .imports(db)
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    for import in imports {
        let parts = import.split('.').collect::<Vec<_>>();
        if resolve(db, module, &parts).is_none() {
            Diagnostic {
                message: format!("module not found: {}", import),
                file: file.name(db).to_string(),
            }
            .accumulate(db);
        }
    }
}

#[salsa::tracked]
fn compile_project<'db>(db: &'db dyn Database, project: Project) {
    let module = get_project_module(db, project);
    for file in project.files(db) {
        check_file(db, module, file);
    }
}
