//! Create the `.ready-set.toml` project meta file.

/// Render the meta file content.
#[must_use]
pub fn render(_cargo_workspace: bool) -> String {
    String::from(
        "[ready-set]\n\
         schema_version = 2\n\
         profile = \"rust-workspace\"\n\
         \n\
         [capabilities.workspace]\n\
         relevance = \"required\"\n\
         provider = \"rust\"\n\
         \n\
         [capabilities.toolchain]\n\
         relevance = \"required\"\n\
         provider = \"rust\"\n\
         \n\
         [capabilities.formatting]\n\
         relevance = \"required\"\n\
         provider = \"rust\"\n\
         \n\
         [capabilities.linting]\n\
         relevance = \"required\"\n\
         provider = \"rust\"\n",
    )
}
