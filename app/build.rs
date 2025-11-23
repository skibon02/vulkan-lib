use std::{fs, path::Path, process::Command};

fn main() {
    let shader_dir = "shaders";
    let output_dir = format!("{}/compiled", shader_dir);

    // create compiled shaders dir
    if let Err(e) = fs::create_dir_all(&output_dir) {
        eprintln!("Failed to create output directory: {}", e);
        std::process::exit(1);
    }

    // compile shaders using `glslc`
    let paths = fs::read_dir(shader_dir).expect("Failed to read shader directory");
    for path in paths {
        let path = path.expect("Failed to read entry").path();

        if let Some(ext) = path.extension() {
            if ext == "vert" || ext == "frag" {
                let output_file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.replace('.', "_") + ".spv")
                    .expect("Failed to generate output file name");
                let output_path = Path::new(&output_dir).join(output_file);

                // Компилируем шейдер с помощью glslc
                let status = Command::new("glslc")
                    .arg(&path)
                    .arg("-o")
                    .arg(&output_path)
                    .status();

                match status {
                    Ok(status) if status.success() => {
                    }
                    _ => {
                        eprintln!("Error: glslc is required but not installed or shader compilation failed.");
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    println!("cargo:rerun-if-changed=shaders");
}
