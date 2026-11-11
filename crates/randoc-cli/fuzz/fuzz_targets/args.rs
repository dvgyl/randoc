#![no_main]
//! Parses arbitrary arguments and applies them to the settings without file access.

use libfuzzer_sys::fuzz_target;
use randoc_cli::cli::Environment;
use randoc_cli::settings::Settings;

struct NoFiles;

impl Environment for NoFiles {
    fn warn(&mut self, _: &str) {}
    fn apply_defaults(&mut self, _: &mut Settings, _: &str) -> randoc::Result<()> {
        Ok(())
    }
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let args: Vec<String> = text.split('\0').map(str::to_owned).collect();
    let _ = randoc_cli::settings_from_args(&args, &mut NoFiles);
});
