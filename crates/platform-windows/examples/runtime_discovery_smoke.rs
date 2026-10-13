#![cfg_attr(windows, windows_subsystem = "console")]

//! Read-only Windows runtime discovery smoke. No state, PATH, service, or
//! runtime files are modified; the output is suitable for acceptance evidence.

#[cfg(windows)]
fn main() {
    use cxszn_menv_platform_windows::{
        WindowsMysqlProvider, WindowsNodeProvider, WindowsPhpProvider, WindowsPythonProvider,
    };
    use cxszn_menv_ports::{RuntimeProvider, ScanRequest, VerifyPathRequest};
    use std::path::PathBuf;

    let request = ScanRequest::default();
    let php = WindowsPhpProvider;
    let node = WindowsNodeProvider;
    let python = WindowsPythonProvider;
    let mysql = WindowsMysqlProvider;
    let providers: [(&str, &dyn RuntimeProvider); 4] = [
        ("php", &php),
        ("node", &node),
        ("python", &python),
        ("mysql", &mysql),
    ];
    let mut results = serde_json::Map::new();
    for (family, provider) in providers {
        let value = match provider.scan(&request) {
            Ok(mut records) => {
                let explicit_variable = match family {
                    "php" => "MENV_TEST_PHP_PATH",
                    "node" => "MENV_TEST_NODE_PATH",
                    "python" => "MENV_TEST_PYTHON_PATH",
                    "mysql" => "MENV_TEST_MYSQL_PATH",
                    _ => "",
                };
                if let Some(path) = std::env::var_os(explicit_variable) {
                    if let Ok(record) = provider.verify_path(&VerifyPathRequest {
                        family: provider.family(),
                        path: PathBuf::from(path),
                    }) {
                        records.push(record);
                    }
                }
                serde_json::to_value(records).expect("serialize runtime records")
            }
            Err(error) => serde_json::json!({ "error": error.to_string() }),
        };
        results.insert(family.to_owned(), value);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&results).expect("serialize discovery evidence")
    );
}

#[cfg(not(windows))]
fn main() {
    eprintln!("runtime_discovery_smoke requires Windows");
    std::process::exit(101);
}
