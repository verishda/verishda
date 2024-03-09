use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
use anyhow::Result;


// install custom URI scheme
// https://www.oauth.com/oauth2-servers/redirect-uris/redirect-uris-native-apps/
// https://brockallen.com/2018/01/20/native-oidc-client-sample-for-windows-that-uses-custom-uri-scheme-handler/

use std::env;

const ROOT_KEY_PATH: &str = "Software\\Classes";
const CUSTOM_URI_SCHEME_KEY_VALUE_NAME: &str = "";
const SHELL_KEY_NAME: &str = "shell";
const OPEN_KEY_NAME: &str = "open";
const COMMAND_KEY_NAME: &str = "command";
const COMMAND_KEY_VALUE_NAME: &str = "";
const URL_PROTOCOL_VALUE_NAME: &str = "URL Protocol";
const URL_PROTOCOL_VALUE_VALUE: &str = "";


fn custom_uri_scheme_key_path(custom_uri_scheme: &str) -> String {
    format!("{}\\{}", ROOT_KEY_PATH, custom_uri_scheme)
}

fn custom_uri_scheme_key_value_value(custom_uri_scheme: &str) -> String {
    format!("URL:{}", custom_uri_scheme)
}

fn command_key_path(custom_uri_scheme: &str) -> String {
    format!("{}\\{}\\{}", custom_uri_scheme_key_path(custom_uri_scheme), SHELL_KEY_NAME, OPEN_KEY_NAME)
}

fn command_key_value_value(redirect_url_param: &str) -> String {
    let current_exe = env::current_exe().unwrap();
    format!( "\"{}\" {} \"%1\"", redirect_url_param, current_exe.to_str().unwrap())
}
pub (crate) fn register_custom_uri_scheme(uri_scheme: &str, redirect_url_param: &str) -> Result<()> {

    let custom_uri_scheme = uri_scheme;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let classes_key = hkcu.open_subkey_with_flags(ROOT_KEY_PATH, KEY_ALL_ACCESS).unwrap();
    let (root, _) = classes_key.create_subkey(custom_uri_scheme).unwrap();
    root.set_value(CUSTOM_URI_SCHEME_KEY_VALUE_NAME, &custom_uri_scheme_key_value_value(custom_uri_scheme)).unwrap();
    root.set_value(URL_PROTOCOL_VALUE_NAME, &URL_PROTOCOL_VALUE_VALUE).unwrap();

    let (shell, _) = root.create_subkey(SHELL_KEY_NAME).unwrap();
    let (open, _) = shell.create_subkey(OPEN_KEY_NAME).unwrap();
    let (command, _) = open.create_subkey(COMMAND_KEY_NAME).unwrap();
    command.set_value(COMMAND_KEY_VALUE_NAME, &command_key_value_value(redirect_url_param)).unwrap();
    Ok(())
}