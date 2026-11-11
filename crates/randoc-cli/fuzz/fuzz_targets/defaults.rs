#![no_main]
//! Reads arbitrary text as a defaults file.

use libfuzzer_sys::fuzz_target;
use randoc_cli::defaults::{self, Loader};
use randoc_cli::settings::Settings;

struct OneFile(Vec<u8>);

impl Loader for OneFile {
    fn read(&mut self, path: &str) -> std::io::Result<Vec<u8>> {
        if path == "d.yaml" {
            Ok(self.0.clone())
        } else {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        }
    }
    fn user_data_dir(&mut self, _: Option<&str>) -> String {
        "/u".to_owned()
    }
    fn env_var(&mut self, _: &str) -> Option<String> {
        None
    }
    fn warn(&mut self, _: &str) {}
    fn max_depth(&self) -> usize {
        // The harness thread has a small stack.
        64
    }
}

fuzz_target!(|data: &[u8]| {
    let mut settings = Settings::default();
    let _ = defaults::apply(&mut settings, "d.yaml", &mut OneFile(data.to_vec()), &mut Vec::new());
});
