use std::{env, fs, io, path::Path, path::PathBuf, process::Command};

fn copy_web_sources(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if matches!(
            name.to_string_lossy().as_ref(),
            "dist" | "node_modules" | "tsconfig.tsbuildinfo"
        ) {
            continue;
        }
        let target = destination.join(&name);
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_web_sources(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(io::Error::other(format!(
                "unsupported Web source entry: {}",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for path in [
        "build.rs",
        "web/index.html",
        "web/package.json",
        "web/package-lock.json",
        "web/tsconfig.json",
        "web/vite.config.ts",
        "web/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| io::Error::other("Cargo manifest path is missing"))?,
    );
    let output_dir = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("Cargo output path is missing"))?,
    );
    println!("cargo:rerun-if-env-changed=NL2SH_RUNTIME_MANIFEST");
    println!("cargo:rerun-if-env-changed=NL2SH_RUNTIME_MANIFEST_SIGNATURE");
    let runtime = env::var_os("NL2SH_RUNTIME_MANIFEST");
    let signature = env::var_os("NL2SH_RUNTIME_MANIFEST_SIGNATURE");
    match (runtime, signature) {
        (Some(runtime), Some(signature)) => {
            let runtime = PathBuf::from(runtime);
            let signature = PathBuf::from(signature);
            println!("cargo:rerun-if-changed={}", runtime.display());
            println!("cargo:rerun-if-changed={}", signature.display());
            fs::copy(runtime, output_dir.join("nl2sh-runtime-extensions.json"))?;
            fs::copy(
                signature,
                output_dir.join("nl2sh-runtime-extensions.json.sig"),
            )?;
        }
        (None, None) => {
            fs::write(output_dir.join("nl2sh-runtime-extensions.json"), [])?;
            fs::write(output_dir.join("nl2sh-runtime-extensions.json.sig"), [])?;
        }
        _ => {
            return Err(io::Error::other(
                "Signed runtime build input requires both manifest and signature",
            )
            .into())
        }
    }
    let source = manifest.join("web");
    let web = output_dir.join("web");
    if web.exists() {
        fs::remove_dir_all(&web)?;
    }
    copy_web_sources(&source, &web)?;
    let output = web.join("dist");
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let install = Command::new(npm).arg("ci").current_dir(&web).status()?;
    if !install.success() {
        return Err(io::Error::other(format!("{npm} ci failed with {install}")).into());
    }
    let build = Command::new(npm)
        .args(["run", "build", "--", "--outDir"])
        .arg(&output)
        .current_dir(&web)
        .status()?;
    if !build.success() {
        return Err(io::Error::other(format!("{npm} run build failed with {build}")).into());
    }
    let output = output
        .to_str()
        .ok_or_else(|| io::Error::other("Cargo output path is not UTF-8"))?;
    println!("cargo:rustc-env=NL2SH_WEB_DIST={output}");
    Ok(())
}
