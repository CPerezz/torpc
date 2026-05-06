fn main() {
    // tauri-build doesn't watch `frontendDist` (the `ui/` directory) by
    // default — `cargo build` after editing app.js / index.html / style.css
    // returns "Finished" instantly without re-embedding the assets, leaving
    // the running binary serving stale frontend code. Watch the dir
    // explicitly so Cargo treats it as a rebuild input.
    println!("cargo:rerun-if-changed=ui");

    tauri_build::build()
}
