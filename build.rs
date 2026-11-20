use std::fs;
use std::path::Path;

fn collect_c_files(dir: &Path, files: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_c_files(&path, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("c") {
                if let Some(path_str) = path.to_str() {
                    println!("cargo:rerun-if-changed={}", path_str);
                    files.push(path_str.to_string());
                }
            }
        }
    }
}

fn main() {
    let mut c_files = Vec::new();
    collect_c_files(Path::new("src/drivers"), &mut c_files);
    collect_c_files(Path::new("src/gpu"), &mut c_files);

    if c_files.is_empty() {
        panic!("Не найдено ни одного .c файла в папках src/drivers или src/gpu!");
    }

    let mut build = cc::Build::new();
    for file in &c_files {
        build.file(file);
    }

    build
    .include("src")
    .include("src/drivers")
    .include("src/gpu")
    .flag("-ffreestanding")
    .flag("-mno-red-zone")
    .flag("-fno-stack-protector")
    .flag("-nostdlib")
    .compile("nvidia_gpu");
}
