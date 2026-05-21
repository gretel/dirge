pub mod bash;
#[cfg(feature = "semantic-c")]
mod c;
#[cfg(feature = "semantic-clojure")]
mod clojure;
#[cfg(feature = "semantic-cpp")]
mod cpp;
#[cfg(feature = "semantic-go")]
mod go;
#[cfg(feature = "semantic-java")]
mod java;
#[cfg(feature = "semantic-python")]
mod python;
#[cfg(feature = "semantic-ruby")]
mod ruby;
#[cfg(feature = "semantic-rust")]
mod rust;
#[cfg(feature = "semantic-ts")]
mod typescript;

#[cfg(feature = "semantic-c")]
pub use c::CAdapter;
#[cfg(feature = "semantic-clojure")]
pub use clojure::ClojureAdapter;
#[cfg(feature = "semantic-cpp")]
pub use cpp::CppAdapter;
#[cfg(feature = "semantic-go")]
pub use go::GoAdapter;
#[cfg(feature = "semantic-java")]
pub use java::JavaAdapter;
#[cfg(feature = "semantic-python")]
pub use python::PythonAdapter;
#[cfg(feature = "semantic-ruby")]
pub use ruby::RubyAdapter;
#[cfg(feature = "semantic-rust")]
pub use rust::RustAdapter;
#[cfg(feature = "semantic-ts")]
pub use typescript::TypescriptAdapter;

use std::path::Path;

use crate::semantic::adapter::LanguageAdapter;

pub struct AdapterRegistry {
    adapters: Vec<Box<dyn LanguageAdapter>>,
}

impl AdapterRegistry {
    pub fn new(adapters: Vec<Box<dyn LanguageAdapter>>) -> Self {
        Self { adapters }
    }

    pub fn find_for_file(&self, file_path: &Path) -> Option<&dyn LanguageAdapter> {
        let ext = file_path.extension()?.to_str()?.to_lowercase();
        self.adapters
            .iter()
            .find(|a| {
                a.extensions()
                    .iter()
                    .any(|e| e.trim_start_matches('.') == ext)
            })
            .map(|a| a.as_ref())
    }

    pub fn all_extensions(&self) -> Vec<String> {
        self.adapters
            .iter()
            .flat_map(|a| {
                a.extensions()
                    .iter()
                    .map(|e| e.trim_start_matches('.').to_string())
            })
            .collect()
    }
}
