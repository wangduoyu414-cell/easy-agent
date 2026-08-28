use std::env;
use std::fs;
use std::path::Path;

use easy_agent::platform::{
    extract_chatgpt_web_installer_tag_for_proof, validate_chatgpt_web_installer_tag_for_proof,
};

fn main() {
    let path = env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("usage: chatgpt_windows_web_installer_proof <installer.exe>"));
    let bytes = fs::read(Path::new(&path)).expect("read Microsoft web installer");
    let tag = extract_chatgpt_web_installer_tag_for_proof(&bytes)
        .expect("extract signed Microsoft Store tag");
    validate_chatgpt_web_installer_tag_for_proof(
        &tag,
        "9PLM9XGG6VKS",
        "OpenAI.Codex_2p2nqsd0c76g0",
    )
    .expect("validate ChatGPT product binding");
    println!("ChatGPT Microsoft web installer product binding verified");
}
