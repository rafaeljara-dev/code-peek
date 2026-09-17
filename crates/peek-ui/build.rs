fn main() {
    let builder = std::thread::Builder::new().stack_size(64 * 1024 * 1024);
    let handler = builder
        .spawn(|| {
            let debug = std::env::var("PROFILE").is_ok_and(|profile| profile == "debug");
            let config = slint_build::CompilerConfiguration::new().with_debug_info(debug);
            slint_build::compile_with_config("ui/app.slint", config)
                .expect("Slint compilation failed");
        })
        .unwrap();
    handler.join().expect("Build thread panicked");
}
